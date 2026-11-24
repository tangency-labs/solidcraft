//! Tooth outlines as profiles: a whole outline as one loop, a cutter's section of one tooth space,
//! and a chamfer's section and its plane, each against what it must be, and hostile numbers
//! refused in words or answered plainly, never a panic.

use std::f64::consts::{FRAC_PI_2, PI};

use super::common::{refusal, HOSTILE, HOSTILE_COUNTS};
use crate::eval::Frame;
use crate::math::Vec2;
use crate::prism::profile::Seg2;
use crate::teeth::outline::{space_section, Outline};
use crate::teeth::section::{
    chamfer_plane, chamfer_section, cutter_section, outline_loop, EndChamfer, MOST_SPACE,
};
use crate::teeth::shaft_spline::{self, SplineForm, SplineMake, SplineNumbers};
use crate::teeth::{Piece, Role};
use crate::Vec3;

/// A straight-sided shaft of six teeth, 28 over 23 mm, 6 mm wide.
fn straight() -> Outline {
    let n = SplineNumbers {
        form: SplineForm::Straight,
        make: SplineMake::Shaft,
        teeth: 6,
        length: 20.0,
        major_diameter: Some(28.0),
        minor_diameter: Some(23.0),
        tooth_width: Some(6.0),
        side_clearance: Some(0.05),
        major_clearance: Some(0.3),
        minor_clearance: Some(0.2),
        ..SplineNumbers::default()
    };
    shaft_spline::plan(&n)
        .expect("the straight-sided shaft")
        .pair
        .first
        .outline
}

/// An involute shaft of twenty teeth of module 1.5: its flanks and fillets are splines.
fn involute() -> Outline {
    let n = SplineNumbers {
        form: SplineForm::Involute,
        make: SplineMake::Shaft,
        teeth: 20,
        length: 20.0,
        module: Some(1.5),
        side_clearance: Some(0.06),
        tip_clearance: Some(0.2),
        ..SplineNumbers::default()
    };
    shaft_spline::plan(&n)
        .expect("the involute shaft")
        .pair
        .first
        .outline
}

/// Whether two points are within a thousand-millionth of a millimetre.
fn near(a: Vec2, b: Vec2) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

#[test]
fn an_outline_is_one_loop_with_its_pieces_in_order() {
    for outline in [straight(), involute()] {
        let teeth = outline.teeth();
        let (_, top) = outline.radii();
        let whole = outline_loop(teeth, outline.pitch_angle(), outline.tooth(), top, 0.0)
            .expect("the loop");
        // Every tooth's pieces, each covering its own segments, in the loop's order.
        assert_eq!(whole.pieces.len(), outline.tooth().len() * teeth as usize);
        let mut next = 0;
        for piece in &whole.pieces {
            assert_eq!(piece.first, next);
            next += piece.segs;
        }
        assert_eq!(next, whole.segs.len());
        // Each tooth's top, one turn of the pitch on from the one before.
        let tops: Vec<f64> = whole
            .pieces
            .iter()
            .filter(|p| p.top)
            .map(|p| p.middle)
            .collect();
        assert!(tops.len() >= teeth as usize, "{}", tops.len());
        // A turned loop: its first tooth's top moves with it.
        let turned = outline_loop(teeth, outline.pitch_angle(), outline.tooth(), top, 0.25)
            .expect("the turned loop");
        let first = |l: &crate::teeth::section::OutlineLoop| {
            l.pieces.iter().find(|p| p.top).map(|p| p.middle)
        };
        let (a, b) = (first(&whole).unwrap(), first(&turned).unwrap());
        assert!((b - a - 0.25).abs() < 1e-12, "{a} {b}");
        // Each joint is, bit for bit, where a free-form segment ends.
        let ends: Vec<Vec2> = whole
            .segs
            .iter()
            .filter_map(|s| match *s {
                Seg2::Cubic { p3, .. } => Some(p3),
                Seg2::Conic { b, .. } => Some(b),
                Seg2::Bezier { n, p } => p.get(usize::from(n)).copied(),
                Seg2::Line { .. } | Seg2::Arc { .. } => None,
            })
            .collect();
        assert!(whole.joints.iter().all(|j| ends.contains(j)));
    }
}

#[test]
fn an_outline_that_is_no_profile_is_refused_in_words() {
    let outline = straight();
    let (_, top) = outline.radii();
    for teeth in [0, crate::teeth::MAX_TEETH + 1] {
        let refused = outline_loop(teeth, 1.0, outline.tooth(), top, 0.0);
        assert!(refused.is_err(), "{teeth}");
    }
    let points = vec![Piece::Curve {
        points: vec![[1.0, 0.0], [0.0, 1.0]],
        role: Role::Flank,
    }];
    assert_eq!(
        refusal(&outline_loop(4, FRAC_PI_2, &points, 1.0, 0.0)),
        Some((
            None,
            "the spline's outline cannot be made into a profile: a curve of the outline was not \
             fitted"
                .to_string()
        ))
    );
    let broken = vec![Piece::Spline {
        degree: 3,
        knots: Vec::new(),
        control: vec![[1.0, 0.0], [0.0, 1.0]],
        role: Role::Flank,
    }];
    assert_eq!(
        refusal(&outline_loop(4, FRAC_PI_2, &broken, 1.0, 0.0)),
        Some((
            None,
            "the spline's outline cannot be made into a profile: the knots and control points do \
             not describe a spline"
                .to_string()
        ))
    );
    // Hostile numbers: a refusal or a loop, never a panic.
    for &teeth in &HOSTILE_COUNTS {
        for &h in &HOSTILE {
            let _ = outline_loop(teeth.min(50), h, outline.tooth(), h, h);
        }
    }
}

#[test]
fn a_cutter_section_follows_the_space_and_holds_no_arc() {
    let outline = straight();
    let (root, top) = outline.radii();
    let space = space_section(&outline, 0.0, top + 0.5).expect("a space");
    let (after, before) = space.corners;
    let cut = cutter_section(
        &space.floor,
        after,
        before,
        root,
        5.0,
        top + 0.5,
        Vec3::ZERO,
    )
    .expect("the section");
    assert_eq!(cut.spans.len(), space.floor.len());
    assert_eq!(cut.spans.iter().sum::<usize>() + 3, cut.segs.len());
    assert!(cut.segs.iter().all(|s| !matches!(s, Seg2::Arc { .. })));
    // The last three are the walls and the top, from the corner before round to the corner after.
    let lines: Vec<(Vec2, Vec2)> = cut
        .segs
        .iter()
        .rev()
        .take(3)
        .filter_map(|s| match *s {
            Seg2::Line { a, b } => Some((a, b)),
            _ => None,
        })
        .collect();
    assert_eq!(lines.len(), 3);
    assert!(near(lines[2].0, Vec2::new(before[0], before[1])));
    assert!(near(lines[0].1, Vec2::new(after[0], after[1])));
    // The middle line lies between the corners; the corners lie mirrored about it.
    assert!(
        (cut.from[1] + cut.to[1]).abs() < 1e-9,
        "{:?} {:?}",
        cut.from,
        cut.to
    );
    let (s, c) = cut.middle.sin_cos();
    assert!(near(
        cut.axis_at,
        Vec2::new((root + 5.0) * c, (root + 5.0) * s)
    ));
    assert!(near(cut.axis_dir, Vec2::new(-s, c)));
}

#[test]
fn a_space_too_wide_for_the_cutter_is_refused_naming_it() {
    // The corners 2 x (pi/2 + 0.6) apart, wider than the widest space taken.
    const { assert!(PI + 1.2 > MOST_SPACE) };
    let after = [(PI / 2.0 + 0.6).cos() * 10.0, (PI / 2.0 + 0.6).sin() * 10.0];
    let before = [
        (-PI / 2.0 - 0.6).cos() * 10.0,
        (-PI / 2.0 - 0.6).sin() * 10.0,
    ];
    let refused = cutter_section(&[], after, before, 8.0, 5.0, 11.0, Vec3::ZERO);
    let (key, words) = refusal(&refused).expect("a refusal");
    assert_eq!(key, Some("cutter_diameter"));
    assert_eq!(
        words,
        "a tooth space is too wide to end in the cutter's curve: leave out `cutter_diameter` (the \
         spaces then end square) or use more teeth"
    );
    // Hostile numbers: a refusal or a section, never a panic.
    for &h in &HOSTILE {
        let _ = cutter_section(&[], [h, 1.0], [1.0, h], h, h, h, Vec3::new(h, h, h));
        let arc = [Piece::Arc {
            centre: [0.0, 0.0],
            radius: 10.0,
            start: 0.1,
            sweep: h,
            role: Role::Root,
        }];
        let _ = cutter_section(&arc, [10.0, 1.0], [10.0, -1.0], 9.0, 2.0, h, Vec3::ZERO);
    }
}

#[test]
fn a_chamfer_section_takes_off_the_corner() {
    let chamfer = EndChamfer {
        radius: 10.0,
        radial: 1.0,
        axial: 1.0,
        past: 0.0,
        flush: None,
    };
    let shaft = chamfer_section(&chamfer, 0.1, true).expect("a shaft's chamfer");
    assert_eq!(shaft.slope, 2);
    let want = [
        Vec2::new(8.9, -0.1),
        Vec2::new(10.1, -0.1),
        Vec2::new(10.1, 1.1),
    ];
    assert_eq!(shaft.points.len(), 3);
    for (p, w) in shaft.points.iter().zip(want) {
        assert!(near(*p, w), "{p:?} {w:?}");
    }
    let bore = chamfer_section(&chamfer, 0.1, false).expect("a bore's chamfer");
    assert_eq!((bore.points.len(), bore.slope), (3, 1));
    // At a neck: the tool stops at the neck's surface.
    let neck = chamfer_section(
        &EndChamfer {
            flush: Some(9.5),
            ..chamfer
        },
        0.1,
        true,
    )
    .expect("a chamfer at a neck");
    assert_eq!((neck.points.len(), neck.slope), (4, 2));
    assert!(near(neck.points[0], Vec2::new(9.5, 0.0)));
    // A neck as large as the face leaves no corner; numbers that are not finite give none.
    let no_corner = EndChamfer {
        flush: Some(10.5),
        ..chamfer
    };
    assert!(chamfer_section(&no_corner, 0.1, true).is_none());
    for &h in &HOSTILE {
        for outside in [true, false] {
            let odd = EndChamfer {
                radius: h,
                radial: h,
                axial: 1.0,
                past: h,
                flush: Some(h),
            };
            if let Some(section) = chamfer_section(&odd, h, outside) {
                assert!(section
                    .points
                    .iter()
                    .all(|p| p.x > 0.0 && p.x.is_finite() && p.y.is_finite()));
            }
        }
    }
}

#[test]
fn a_chamfer_plane_runs_through_the_axis() {
    let end = Frame {
        origin: [1.0, 2.0, 3.0],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    };
    let through = chamfer_plane(&end, -1.0, FRAC_PI_2);
    assert_eq!(through.origin, [1.0, 2.0, 3.0]);
    let x = Vec3::from_array(through.x_axis);
    assert!((x - Vec3::Y).length() < 1e-15, "{x:?}");
    assert_eq!(through.y_axis, [0.0, 0.0, -1.0]);
    for &h in &HOSTILE {
        let _ = chamfer_plane(&end, h, h);
    }
}
