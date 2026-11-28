//! Tooth outlines as profiles: a whole outline as one loop, a cutter's section of one tooth space,
//! the whole cross-section of a space of any width, and a chamfer's section and its plane, each
//! against what it must be, and hostile numbers refused as data or answered plainly, never a
//! panic.

use crate::math::det;
use std::f64::consts::{FRAC_PI_2, PI, TAU};

use super::common::{failure, HOSTILE, HOSTILE_COUNTS};
use crate::error::teeth::{ProfileWhy, SplineQuantity, TeethFailure, Why};
use crate::eval::Frame;
use crate::math::Vec2;
use crate::prism::profile::{Seg2, MAX_BEZIER_CTRL};
use crate::teeth::outline::{space_section, Outline};
use crate::teeth::section::{
    chamfer_plane, chamfer_section, cutter_section, outline_loop, space_region, tooth_at,
    ChamferRefusal, EndChamfer, MOST_SPACE,
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

/// The number a refused call names and its words.
fn said<T>(r: &crate::Result<T>) -> Option<(Option<SplineQuantity>, String)> {
    failure(r).map(|f| (f.quantity(), f.to_string()))
}

/// Where a profile segment starts and ends.
fn ends(s: &Seg2) -> (Vec2, Vec2) {
    match *s {
        Seg2::Line { a, b } | Seg2::Conic { a, b, .. } => (a, b),
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let at = |angle: f64| {
                let (s, c) = det::sin_cos(angle);
                Vec2::new(center.x + radius * c, center.y + radius * s)
            };
            (at(start), at(start + sweep))
        }
        Seg2::Cubic { p0, p3, .. } => (p0, p3),
        Seg2::Bezier { n, p } => (p[0], p[usize::from(n)]),
    }
}

/// Whether the segments run on from one to the next and the last back to the first.
fn closed(segs: &[Seg2]) -> bool {
    segs.iter()
        .zip(segs.iter().cycle().skip(1))
        .all(|(s, t)| near(ends(s).1, ends(t).0))
}

/// A piece of a loop is numbered by the tooth whose top is nearest its angle: a little either
/// side of a top, a whole turn on or back, and counted from where tooth 0 stands.
#[test]
fn an_angle_belongs_to_the_tooth_whose_top_is_nearest() {
    // Eight teeth: a pitch is an eighth of a turn, exactly (a division by a power of two).
    let pitch = TAU / 8.0;
    for k in 0..8_u32 {
        let top = f64::from(k) * pitch;
        assert_eq!(tooth_at(8, 0.0, top), k);
        assert_eq!(tooth_at(8, 0.0, top + 0.4 * pitch), k);
        assert_eq!(tooth_at(8, 0.0, top - 0.4 * pitch), k);
        assert_eq!(tooth_at(8, 0.0, top + TAU), k);
        assert_eq!(tooth_at(8, 0.0, top - TAU), k);
        // Tooth 0 at 0.3 radians (a master spline turns the count with it).
        assert_eq!(tooth_at(8, 0.3, top + 0.3), k);
    }
    // Past the middle of a space the next tooth is nearer.
    assert_eq!(tooth_at(8, 0.0, 0.6 * pitch), 1);
    assert_eq!(tooth_at(8, 0.0, -0.6 * pitch), 7);
    // No teeth, and numbers that are not numbers: tooth 0 or a tooth of the count, never a
    // panic.
    assert_eq!(tooth_at(0, 0.0, 1.0), 0);
    for &h in &HOSTILE {
        assert!(tooth_at(8, h, 1.0) < 8, "{h}");
        assert!(tooth_at(8, 0.0, h) < 8, "{h}");
    }
    for &n in &HOSTILE_COUNTS {
        let k = tooth_at(n, 0.0, 1.0);
        assert!(k == 0 || k < n, "{n}: {k}");
    }
}

#[test]
fn an_outline_is_one_loop_with_its_pieces_in_order() {
    for outline in [straight(), involute()] {
        let teeth = outline.teeth();
        let (_, top) = outline.radii();
        let tol = outline.tolerance();
        let whole = outline_loop(teeth, outline.pitch_angle(), outline.tooth(), top, 0.0, tol)
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
        let turned = outline_loop(
            teeth,
            outline.pitch_angle(),
            outline.tooth(),
            top,
            0.25,
            tol,
        )
        .expect("the turned loop");
        let first = |l: &crate::teeth::section::OutlineLoop| {
            l.pieces.iter().find(|p| p.top).map(|p| p.middle)
        };
        let (a, b) = (first(&whole).unwrap(), first(&turned).unwrap());
        assert!((b - a - 0.25).abs() < 1e-12, "{a} {b}");
        // Each joint is, bit for bit, where a free-form segment ends.
        let free_ends: Vec<Vec2> = whole
            .segs
            .iter()
            .filter_map(|s| match *s {
                Seg2::Cubic { p3, .. } => Some(p3),
                Seg2::Conic { b, .. } => Some(b),
                Seg2::Bezier { n, p } => p.get(usize::from(n)).copied(),
                Seg2::Line { .. } | Seg2::Arc { .. } => None,
            })
            .collect();
        assert!(whole.joints.iter().all(|j| free_ends.contains(j)));
        assert!(closed(&whole.segs));
    }
}

#[test]
fn an_outline_that_is_no_profile_is_refused_as_data() {
    let outline = straight();
    let (_, top) = outline.radii();
    for teeth in [0, crate::teeth::MAX_TEETH + 1] {
        let refused = outline_loop(teeth, 1.0, outline.tooth(), top, 0.0, 1e-3);
        assert!(refused.is_err(), "{teeth}");
    }
    let points = vec![Piece::Curve {
        points: vec![[1.0, 0.0], [0.0, 1.0]],
        role: Role::Flank,
    }];
    let refused = outline_loop(4, FRAC_PI_2, &points, 1.0, 0.0, 1e-3);
    assert_eq!(
        failure(&refused),
        Some(TeethFailure::Refused(
            crate::error::teeth::PlanRefusal::NotAProfile(ProfileWhy::NotFitted)
        ))
    );
    assert_eq!(
        said(&refused),
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
        said(&outline_loop(4, FRAC_PI_2, &broken, 1.0, 0.0, 1e-3)),
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
            let _ = outline_loop(teeth.min(50), h, outline.tooth(), h, h, h);
        }
    }
}

/// A spline of a degree higher than a profile segment holds is never refused: it becomes Bezier
/// pieces a profile holds, within the tolerance, and the loop still closes. Up to the highest
/// degree the kernel's curves have ([`crate::nurbs::knots::MAX_DEGREE`]): past it the kernel holds
/// no such curve, and the knots and control points are not a spline.
#[test]
fn a_spline_of_a_high_degree_becomes_pieces_a_profile_holds() {
    for degree in [MAX_BEZIER_CTRL, 12, crate::nurbs::knots::MAX_DEGREE] {
        // One Bezier piece of the parabola y = x^2 / 10 over 0..10, its control points on it:
        // the curve is a quadratic, whatever the degree it is held in.
        let control: Vec<[f64; 2]> = (0..=degree)
            .map(|i| {
                let x = 10.0 * i as f64 / degree as f64;
                [x, x * x / 10.0]
            })
            .collect();
        let knots: Vec<f64> = (0..2 * (degree + 1))
            .map(|i| if i <= degree { 0.0 } else { 1.0 })
            .collect();
        let tooth = [
            Piece::Spline {
                degree,
                knots,
                control,
                role: Role::Flank,
            },
            Piece::Line {
                from: [10.0, 10.0],
                to: [0.0, 0.0],
                role: Role::Root,
            },
        ];
        let whole = outline_loop(1, TAU, &tooth, 20.0, 0.0, 1e-6).expect("a high degree");
        for s in &whole.segs {
            if let Seg2::Bezier { n, .. } = s {
                assert!(usize::from(*n) < MAX_BEZIER_CTRL, "{degree}: {n}");
            }
        }
        assert!(closed(&whole.segs), "{degree}");
    }
}

#[test]
fn a_cutter_section_follows_the_space_and_keeps_its_arcs() {
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
        outline.tolerance(),
    )
    .expect("the section")
    .expect("a space narrow enough for one cutter");
    assert_eq!(cut.spans.len(), space.floor.len());
    assert_eq!(cut.spans.iter().sum::<usize>() + 3, cut.segs.len());
    // Each arc of the floor is one arc of the section, as it is: never pieces bent off it.
    let mut at = 0;
    for (piece, span) in space.floor.iter().zip(&cut.spans) {
        if let Piece::Arc {
            centre,
            radius,
            start,
            sweep,
            ..
        } = piece
        {
            assert_eq!(*span, 1);
            assert_eq!(
                cut.segs[at],
                Seg2::Arc {
                    center: Vec2::new(centre[0], centre[1]),
                    radius: *radius,
                    start: *start,
                    sweep: *sweep,
                }
            );
        }
        at += span;
    }
    assert!(space
        .floor
        .iter()
        .any(|piece| matches!(piece, Piece::Arc { .. })));
    assert!(closed(&cut.segs));
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
    let (s, c) = det::sin_cos(cut.middle);
    assert!(near(
        cut.axis_at,
        Vec2::new((root + 5.0) * c, (root + 5.0) * s)
    ));
    assert!(near(cut.axis_dir, Vec2::new(-s, c)));
}

/// A space wider than one cutter's section reaches round is no error: it has no section, and its
/// whole cross-section ends it square, for any width below a full turn.
#[test]
fn a_space_too_wide_for_one_cutter_ends_square() {
    // The corners 2 x (pi/2 + 0.6) apart, wider than one cutter's section reaches round.
    const { assert!(PI + 1.2 > MOST_SPACE) };
    let after = [
        det::cos(PI / 2.0 + 0.6) * 10.0,
        det::sin(PI / 2.0 + 0.6) * 10.0,
    ];
    let before = [
        det::cos(-PI / 2.0 - 0.6) * 10.0,
        det::sin(-PI / 2.0 - 0.6) * 10.0,
    ];
    let none = cutter_section(&[], after, before, 8.0, 5.0, 11.0, 1e-3).expect("no refusal");
    assert_eq!(none, None);
    // The floor here is the circle of radius 10 across the space, clockwise from the corner after
    // to the corner before.
    let floor = [Piece::Arc {
        centre: [0.0, 0.0],
        radius: 10.0,
        start: PI / 2.0 + 0.6,
        sweep: -(PI + 1.2),
        role: Role::Root,
    }];
    let region = space_region(&floor, after, before, 11.0, 1e-3).expect("the region");
    assert_eq!(region.spans, vec![1]);
    assert_eq!(region.segs.len(), 4);
    assert!(closed(&region.segs));
    // Out from the corner before, round the circle of radius 11, in to the corner after.
    let Seg2::Arc {
        center,
        radius,
        sweep,
        ..
    } = region.segs[2]
    else {
        panic!("the arc of the circle: {:?}", region.segs[2]);
    };
    assert_eq!((center, radius), (Vec2::ZERO, 11.0));
    assert!((sweep - (PI + 1.2)).abs() < 1e-12, "{sweep}");
    let (out, _) = ends(&region.segs[3]);
    assert!(
        near(out, Vec2::new(after[0] * 1.1, after[1] * 1.1)),
        "{out:?}"
    );
    // Nearly a full turn: still a region.
    let wide_after = [10.0 * det::cos(0.05_f64), 10.0 * det::sin(0.05_f64)];
    let wide_before = [10.0 * det::cos(0.1_f64), 10.0 * det::sin(0.1_f64)];
    let nearly = space_region(&[], wide_after, wide_before, 12.0, 1e-3).expect("a wide region");
    let Seg2::Arc { sweep, .. } = nearly.segs[1] else {
        panic!("the arc of the circle: {:?}", nearly.segs[1]);
    };
    assert!((sweep - (TAU - 0.05)).abs() < 1e-12, "{sweep}");
    // A circle inside a corner, and corners of no width: refused as data.
    let inside = space_region(&floor, after, before, 9.0, 1e-3);
    assert!(matches!(
        failure(&inside),
        Some(TeethFailure::OutOfRange { what: "radius", .. })
    ));
    let none_wide = space_region(&[], after, after, 11.0, 1e-3);
    assert_eq!(
        failure(&none_wide),
        Some(TeethFailure::Invalid(Why::TopsMeet))
    );
    // Hostile numbers: a refusal, a section or none, never a panic.
    for &h in &HOSTILE {
        let _ = cutter_section(&[], [h, 1.0], [1.0, h], h, h, h, h);
        let _ = space_region(&[], [h, 1.0], [1.0, h], h, h);
        let arc = [Piece::Arc {
            centre: [0.0, 0.0],
            radius: 10.0,
            start: 0.1,
            sweep: h,
            role: Role::Root,
        }];
        let _ = cutter_section(&arc, [10.0, 1.0], [10.0, -1.0], 9.0, 2.0, h, 1e-3);
        let _ = space_region(&arc, [10.0, 1.0], [10.0, -1.0], h, 1e-3);
    }
}

/// The whole cross-section of a space of an involute shaft: its flanks and fillets as Bezier
/// pieces, closed by the lines out and the arc of the circle.
#[test]
fn an_involute_space_region_closes() {
    let outline = involute();
    let (_, top) = outline.radii();
    let space = space_section(&outline, 0.0, top + 0.5).expect("a space");
    let (after, before) = space.corners;
    let region = space_region(&space.floor, after, before, top + 0.5, outline.tolerance())
        .expect("the region");
    assert_eq!(region.spans.len(), space.floor.len());
    assert_eq!(region.spans.iter().sum::<usize>() + 3, region.segs.len());
    assert!(closed(&region.segs));
    let cut = cutter_section(
        &space.floor,
        after,
        before,
        outline.radii().0,
        5.0,
        top + 0.5,
        outline.tolerance(),
    )
    .expect("the section")
    .expect("a narrow space");
    // The floor is the same in both.
    let floor: usize = region.spans.iter().sum();
    assert_eq!(region.segs[..floor], cut.segs[..floor]);
    assert_eq!(region.joints, cut.joints);
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
    // Each number that rules a section out is named.
    let refusals = [
        (
            EndChamfer {
                flush: Some(10.5),
                ..chamfer
            },
            ChamferRefusal::NoCorner,
        ),
        (
            EndChamfer {
                radial: f64::NAN,
                ..chamfer
            },
            ChamferRefusal::NotFinite,
        ),
        (
            EndChamfer {
                axial: 0.0,
                ..chamfer
            },
            ChamferRefusal::NoSize,
        ),
        (
            EndChamfer {
                radial: 10.5,
                ..chamfer
            },
            ChamferRefusal::ReachesAxis,
        ),
    ];
    for (bad, why) in refusals {
        assert_eq!(chamfer_section(&bad, 0.1, true), Err(why), "{bad:?}");
    }
    assert_eq!(
        chamfer_section(&chamfer, f64::INFINITY, true),
        Err(ChamferRefusal::NotFinite)
    );
    for &h in &HOSTILE {
        for outside in [true, false] {
            let odd = EndChamfer {
                radius: h,
                radial: h,
                axial: 1.0,
                past: h,
                flush: Some(h),
            };
            if let Ok(section) = chamfer_section(&odd, h, outside) {
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
