//! An arm joined to a ring whose outer rim is rounded (R30/R24, 6 mm tall, 2 mm rounds on its
//! outer edges), its end just reaching the rim, just touching it, or just clear of it.
//!
//! The arm's convex end cutting 0.03–0.2 mm into the rim failed after minutes ("implausible
//! result volume"): the intersection was right, but the membership samples in the overlap box,
//! no thicker than the meshes' chords, read the facetings instead of the result. A gap of a
//! hundredth of a millimetre took most of a minute to intersect and came back unchecked
//! ("empty result"), and a hundredth of a millimetre of overlap joined as two shells, the
//! sampled shortcut taking the bodies for apart. Faces lying on one cylinder failed only after
//! every nudge had run every retry again.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::f64::consts::{FRAC_PI_2, PI};

use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

use crate::distance::{Contact, DistanceShape, contact};
use crate::*;

/// The ring with its rounded rim.
fn ring() -> Body {
    let outer = cylinder(Vec3::ZERO, Vec3::Z, 30.0, 6.0).unwrap();
    let bore = cylinder(Vec3::ZERO, Vec3::Z, 24.0, 6.0).unwrap();
    let ring = boolean(&outer, &bore, BoolOp::Cut).unwrap().unwrap();
    fillet(&ring, &[Vec3::new(30.0, 0.0, 6.0), Vec3::new(30.0, 0.0, 0.0)], 2.0).unwrap()
}

/// The rim's radius at height `z`.
fn rim(z: f64) -> f64 {
    let past = (2.0 - z).max(z - 4.0).max(0.0);
    28.0 + (4.0 - past * past).max(0.0).sqrt()
}

/// An arm 10 mm wide, 40 mm long and as tall as the ring, its convex end (radius 5) `gap` off
/// the rim.
fn convex_arm(gap: f64) -> Body {
    let cx = 35.0 + gap;
    let p = Vec2::new;
    let line = |a: Vec2, b: Vec2| Seg2::Line { a, b };
    let segs = vec![
        line(p(cx, 5.0), p(cx + 40.0, 5.0)),
        line(p(cx + 40.0, 5.0), p(cx + 40.0, -5.0)),
        line(p(cx + 40.0, -5.0), p(cx, -5.0)),
        Seg2::Arc { center: p(cx, 0.0), radius: 5.0, start: -FRAC_PI_2, sweep: -PI },
    ];
    extrude(&Plane::XY, &[Region2 { outer: Loop2 { segs }.ccw(), holes: vec![] }], 0.0, 6.0).unwrap().remove(0)
}

/// An arm whose concave end hugs the ring, on its axis, `gap` off the rim.
fn concave_arm(gap: f64) -> Body {
    let (r, w) = (30.0 + gap, 5.0);
    let x0 = (r * r - w * w).sqrt();
    let start = (-w).atan2(x0);
    let p = Vec2::new;
    let line = |a: Vec2, b: Vec2| Seg2::Line { a, b };
    let segs = vec![
        line(p(x0, w), p(70.0, w)),
        line(p(70.0, w), p(70.0, -w)),
        line(p(70.0, -w), p(x0, -w)),
        Seg2::Arc { center: p(0.0, 0.0), radius: r, start, sweep: -2.0 * start },
    ];
    extrude(&Plane::XY, &[Region2 { outer: Loop2 { segs }.ccw(), holes: vec![] }], 0.0, 6.0).unwrap().remove(0)
}

/// The volume the convex arm `gap` (negative) into the rim shares with the ring: the lens its
/// end makes with the rim's circle at each height, summed over the height.
fn shared(gap: f64) -> f64 {
    let lens = |big: f64, small: f64, d: f64| {
        if d >= big + small {
            return 0.0;
        }
        let a = big * big * ((d * d + big * big - small * small) / (2.0 * d * big)).acos();
        let b = small * small * ((d * d + small * small - big * big) / (2.0 * d * small)).acos();
        let c = 0.5 * ((-d + small + big) * (d + small - big) * (d - small + big) * (d + small + big)).max(0.0).sqrt();
        a + b - c
    };
    let n = 60_000;
    (0..n).map(|i| lens(rim((f64::from(i) + 0.5) * 6.0 / f64::from(n)), 5.0, 35.0 + gap)).sum::<f64>() * 6.0 / f64::from(n)
}

fn volume(b: &Body) -> f64 {
    measure(b).unwrap().volume
}

fn contact_of(a: &Body, b: &Body) -> Contact {
    let chord = 0.03;
    let (ma, mb) = (a.tessellate_with(chord, true).unwrap(), b.tessellate_with(chord, true).unwrap());
    contact(&DistanceShape::new(a, &ma, chord), &DistanceShape::new(b, &mb, chord), 1e-6).unwrap()
}

/// The arm's end 0.1 mm into the rim: the union, the shared part and the cut, each right
/// (V(A ∪ B) + V(A ∩ B) = V(A) + V(B), V(A − B) = V(A) − V(A ∩ B)), and none of them moved
/// against the other body to get there.
#[test]
fn an_arm_cut_into_a_rounded_rim_joins() {
    let (target, arm) = (ring(), convex_arm(-0.1));
    let (va, vb) = (volume(&target), volume(&arm));
    let run = |op| {
        let (r, report) = capture_boolean_diagnostics(|| boolean(&target, &arm, op));
        let body = r.unwrap_or_else(|e| panic!("{op:?}: {e}")).unwrap_or_else(|| panic!("{op:?} came out empty"));
        assert!(report.warning().is_none(), "{op:?}: {report:?}");
        assert_eq!(body.shell_count(), 1, "{op:?}");
        volume(&body)
    };
    let (vu, vi, vc) = (run(BoolOp::Union), run(BoolOp::Intersect), run(BoolOp::Cut));
    let want = shared(-0.1);
    assert!((vi - want).abs() < want * 0.02, "shared {vi}, expected {want}");
    assert!((vu + vi - va - vb).abs() < 0.25, "union {vu} + shared {vi} against {va} + {vb}");
    assert!((vc + vi - va).abs() < 0.25, "cut {vc} + shared {vi} against {va}");
}

/// A hundredth of a millimetre into the rim, the samples can't see the shared part: the faces
/// crossing each other show it, and the union fuses.
#[test]
fn an_arm_a_hundredth_into_a_rim_fuses() {
    let (target, arm) = (ring(), convex_arm(-0.01));
    assert_eq!(contact_of(&target, &arm), Contact::Crossing);
    let joined = boolean(&target, &arm, BoolOp::Union).unwrap().unwrap();
    assert_eq!(joined.shell_count(), 1);
    let common = boolean(&target, &arm, BoolOp::Intersect).unwrap().expect("they share a little");
    let want = shared(-0.01);
    assert!((volume(&common) - want).abs() < want * 0.1, "shared {}, expected {want}", volume(&common));
}

/// A hair clear of the rim (the convex end 0.001 mm, the concave end 0.01 mm off it on the
/// ring's axis), the meshes pass through each other but the faces stay apart: nothing in
/// common, both side by side in a union, at once and with no retry.
#[test]
fn an_arm_a_hair_off_a_rim_is_apart() {
    let target = ring();
    for (arm, gap) in [(convex_arm(0.001), 0.001), (concave_arm(0.01), 0.01)] {
        let Contact::Apart(d) = contact_of(&target, &arm) else { panic!("{gap}: not apart") };
        assert!((d - gap).abs() < gap * 0.01, "{gap}: apart by {d}");
        let (common, report) = capture_boolean_diagnostics(|| boolean(&target, &arm, BoolOp::Intersect));
        assert!(common.unwrap().is_none(), "{gap}");
        assert!(report.events.is_empty(), "{gap}: {report:?}");
        let (joined, report) = capture_boolean_diagnostics(|| boolean(&target, &arm, BoolOp::Union));
        let joined = joined.unwrap().unwrap();
        assert!(report.events.is_empty(), "{gap}: {report:?}");
        assert_eq!(joined.shell_count(), 2, "{gap}");
        // Measured as one body, at the step of the larger box.
        let sum = volume(&target) + volume(&arm);
        assert!((volume(&joined) - sum).abs() < sum * 1e-5, "{gap}: {} against {sum}", volume(&joined));
    }
}

/// Touching (the convex end tangent to the rim) is neither apart nor crossing.
#[test]
fn an_arm_tangent_to_a_rim_touches() {
    assert_eq!(contact_of(&ring(), &convex_arm(0.0)), Contact::Touching);
}

/// The concave end lying on the rim's cylinder, back to back: no route takes it, and the join
/// says why once its attempts are spent, instead of after every nudge has run every retry.
#[test]
fn a_join_on_a_common_cylinder_says_why() {
    let e = boolean(&ring(), &concave_arm(0.0), BoolOp::Union).expect_err("not supported");
    assert!(e.to_string().contains("lie on one cylinder"), "{e}");
}
