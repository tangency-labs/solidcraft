//! Fillets and chamfers against closed forms (the cases of SolidCraft's `core/blend_tests.rs`,
//! on edge, face and vertex indices). Their own test binary: each holds the process kernel for
//! seconds.
#![allow(
    clippy::disallowed_methods, // closed-form oracles, not ops geometry
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::expect_used
)] // test helpers

#[path = "../../solidcore/tests/support/solidcraft_fixes.rs"]
mod solidcraft_fixes;

mod common;

use common::*;

use std::f64::consts::PI;
use std::io::Read as _;

use solidcore::blend::definition_check::surface_kind_name;
use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::kernel::Kernel;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::query::api_queries::{edge_closest_point, face_bounding_box};
use solidcore::surface::procedural_blend::{BlendRef, ProceduralBlend};
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::api_files::write_brep_to_file;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::EdgeuseId;
use solidcore::topology::{edge, loopuse};
use solidcore::{EdgeId, FaceId, Point2, Vec3};
use solidcore_record::brep::Brep as Record;

use solidcore_ops::sweep::{extrude, loft_to_point, Curve, DraftCorner, SweepFrame};
use solidcore_ops::{
    blend, blend_ordered, blend_with, chamfer_sides, chamfer_sides_facing, edge_faces_along,
    edges_share_wedge, fillet_variable, fillet_variable_from, make_box, make_cone, make_cylinder,
    tangent_chain, Blend, BlendOptions, Body, ChamferSecond, CheckLevel, Corners, Error, FitKind,
    Order, SetBack, Turn, Unblendable,
};

/// The block's top front edge, 30 long, between the top (z = 10) and the front (y = 0).
fn top_front(b: &Body) -> usize {
    edge_at(b, [15.0, 0.0, 10.0])
}

/// Closed forms recorded in
/// `plan/findings/blend.fillet_solver_p5--neighbouring-chamfers-closed-forms.md`.
/// The overlap of the two removed triangular prisms is integrated over depth w.
fn neighbouring_chamfer_measures(l: f64, a: f64, b: f64, c: f64, d: f64) -> (f64, f64) {
    let m = b.min(d);
    let overlap = a * c * (m - m * m * (1.0 / b + 1.0 / d) / 2.0 + m * m * m / (3.0 * b * d));
    let volume = l * l * l - l * a * b / 2.0 - l * c * d / 2.0 + overlap;
    let area = l * l + (l - c) * (l - a) + l * (l - b) - c * (d - b).max(0.0).powi(2) / (2.0 * d)
        + l * l
        - c * d / 2.0
        + l * l
        - a * b / 2.0
        + l * (l - d)
        - a * (b - d).max(0.0).powi(2) / (2.0 * b)
        + a.hypot(b) * (l - c / b * (m - m * m / (2.0 * d)))
        + c.hypot(d) * (l - a / d * (m - m * m / (2.0 * b)));
    (volume, area)
}

#[test]
fn neighbouring_unequal_chamfers_build_in_either_history_order() {
    all_fixes();
    let mut failures = Vec::new();
    for scale in [1.0, 0.5, 2.0] {
        for flip_angle in [true, false] {
            for flip_distances in [false, true] {
                for reverse in [false, true] {
                    let case = (scale, flip_angle, flip_distances, reverse);
                    let b = make_box([0.0; 3], [10.0 * scale; 3]).unwrap();
                    let angle = || {
                        chamfer_sides_facing(
                            &b,
                            &[edge_at(&b, [5.0 * scale, 0.0, 10.0 * scale])],
                            scale,
                            ChamferSecond::Angle(PI / 6.0),
                            [0.0, 0.0, if flip_angle { -1.0 } else { 1.0 }],
                        )
                    };
                    let distances = || {
                        chamfer_sides_facing(
                            &b,
                            &[edge_at(&b, [10.0 * scale, 5.0 * scale, 10.0 * scale])],
                            scale,
                            ChamferSecond::Distance(3.0 * scale),
                            [0.0, 0.0, if flip_distances { -1.0 } else { 1.0 }],
                        )
                    };
                    let result = if reverse {
                        distances().unwrap();
                        angle()
                    } else {
                        angle().unwrap();
                        distances()
                    };
                    if let Err(error) = result {
                        failures.push((case, error));
                        continue;
                    }
                    let depth = scale * (PI / 6.0).tan();
                    let (a, front) = if flip_angle {
                        (depth, scale)
                    } else {
                        (scale, depth)
                    };
                    let (c, right) = if flip_distances {
                        (3.0 * scale, scale)
                    } else {
                        (scale, 3.0 * scale)
                    };
                    let (want_volume, want_area) =
                        neighbouring_chamfer_measures(10.0 * scale, a, front, c, right);
                    assert!(b.check(CheckLevel::All).unwrap().is_valid(), "{case:?}");
                    assert_eq!(b.counts().unwrap()[0], 8, "{case:?}");
                    assert!(rel(volume(&b), want_volume) < 1e-9, "{case:?}");
                    assert!(rel(b.area(1e-8).unwrap(), want_area) < 1e-9, "{case:?}");
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn rounds_and_chamfers_take_their_closed_forms() {
    all_fixes();
    // a round of r 2 along 30: (1 - pi/4) r^2 off per unit length
    let b = block();
    let e = top_front(&b);
    let made = blend(&b, Blend::Round, &[e], &[2.0], &[], Corners::Filled).unwrap();
    assert_eq!(made.dropped, vec![false]);
    assert!(made.holes.is_empty());
    assert!(rel(volume(&b), ABC - (1.0 - PI / 4.0) * 4.0 * 30.0) < 1e-9);
    // an equal chamfer of 2 along 30: d^2 / 2 off per unit length
    let b = block();
    blend(
        &b,
        Blend::Chamfer,
        &[top_front(&b)],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert!(rel(volume(&b), ABC - 2.0 * 30.0) < 1e-9);
    // a chord round of width 2 sqrt 2 between planes at 90 degrees is the round of r 2
    let b = block();
    blend(
        &b,
        Blend::Chord,
        &[top_front(&b)],
        &[2.0 * std::f64::consts::SQRT_2],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert!(rel(volume(&b), ABC - (1.0 - PI / 4.0) * 4.0 * 30.0) < 1e-8);
    // the curvature round builds and takes off the ball's area
    let b = block();
    blend(
        &b,
        Blend::Curvature,
        &[top_front(&b)],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert!(rel(volume(&b), ABC - (1.0 - PI / 4.0) * 4.0 * 30.0) < 1e-6);
}

/// A post on a plate. The aperture starts exactly at the shared endpoint of the foot edges.
fn post_foot(cutout: bool) -> Body {
    use solidcore_ops::{boolean::boolean, BooleanOp};

    let plate = make_box([0.0; 3], [50.0, 50.0, 4.0]).unwrap();
    let post = make_box([0.0, 0.0, 4.0], [10.0, 10.0, 30.0]).unwrap();
    let joined = boolean(plate, post, BooleanOp::Union).unwrap().unwrap();
    if cutout {
        let aperture = make_box([10.0, 10.0, -1.0], [5.0, 5.0, 6.0]).unwrap();
        boolean(joined, aperture, BooleanOp::Subtract)
            .unwrap()
            .unwrap()
    } else {
        joined
    }
}

fn post_foot_edges(body: &Body) -> [usize; 2] {
    [
        edge_at(body, [10.0, 5.0, 4.0]),
        edge_at(body, [5.0, 10.0, 4.0]),
    ]
}

#[test]
fn post_foot_blends_stop_at_a_through_cut_corner() {
    use solidcore_ops::PointClass;
    all_fixes();
    let chamfer = 5.0_f64;
    let round = 2.0_f64;
    for (kind, size, section, diagonal) in [
        (
            Blend::Chamfer,
            chamfer,
            chamfer.powi(2) / 2.0,
            chamfer / 2.0,
        ),
        (
            Blend::Round,
            round,
            round.powi(2) * (1.0 - PI / 4.0),
            round * (1.0 - std::f64::consts::FRAC_1_SQRT_2),
        ),
    ] {
        for reverse in [false, true] {
            let body = post_foot(true);
            let mut edges = post_foot_edges(&body);
            if reverse {
                edges.reverse();
            }
            let made = blend(&body, kind, &edges, &[size], &[], Corners::Filled).unwrap();
            assert_eq!(made.dropped, [false, false]);
            assert!(made.holes.is_empty());
            assert_eq!(body.solid_count().unwrap(), 1);
            assert_eq!(body.to_record().unwrap().0.shells.len(), 1);
            assert!(body.check(CheckLevel::All).unwrap().is_valid());

            // The two ten-unit extrusions occupy disjoint plan strips at the aperture.
            let base = 50.0 * 50.0 * 4.0 + 10.0 * 10.0 * 30.0 - 5.0 * 5.0 * 4.0;
            let expected = base + 20.0 * section;
            let actual = volume(&body);
            assert!(
                rel(actual, expected) < 1e-9,
                "{kind:?}: {actual} != {expected}"
            );

            // The aperture corner below the blend transition and the post corner above it
            // remain distinct edges. Both blends end at the vertex on that vertical line.
            let _ = vertex_at(&body, [10.0, 10.0, 4.0 + size]);
            assert_ne!(
                edge_at(&body, [10.0, 10.0, 2.0]),
                edge_at(&body, [10.0, 10.0, 5.0 + size])
            );
            for (offset, expected) in [
                (diagonal / 2.0, PointClass::Inside),
                (diagonal, PointClass::OnBoundary),
                (diagonal * 1.5, PointClass::Outside),
            ] {
                for point in [
                    [10.0 + offset, 9.5, 4.0 + offset],
                    [9.5, 10.0 + offset, 4.0 + offset],
                ] {
                    assert_eq!(
                        body.classify_point(point).unwrap(),
                        expected,
                        "{kind:?} at {point:?}"
                    );
                }
            }
            for z in [2.0, 4.25] {
                let point = [10.25, 10.25, z];
                assert_eq!(
                    body.classify_point(point).unwrap(),
                    PointClass::Outside,
                    "{kind:?} at {point:?}"
                );
            }
        }
    }
}

#[test]
fn post_foot_blends_leave_the_body_intact_when_the_opening_is_too_short() {
    all_fixes();
    for (kind, fit_kind, section) in [
        (Blend::Chamfer, FitKind::Distance, 25.0 / 2.0),
        (Blend::Round, FitKind::Radius, 25.0 * (1.0 - PI / 4.0)),
    ] {
        let body = post_foot(true);
        let counts = body.counts().unwrap();
        let edges = post_foot_edges(&body);
        let result = blend(&body, kind, &edges, &[6.0], &[], Corners::Filled);
        let Err(Error::TooLarge {
            size,
            kind: reported,
            at: Some(at),
            ..
        }) = result
        else {
            panic!("{result:?}");
        };
        assert_eq!(size, 5.0);
        assert_eq!(reported, fit_kind);
        assert!([[10.0, 5.0, 4.0], [5.0, 10.0, 4.0]].contains(&at));
        assert_eq!(body.counts().unwrap(), counts);
        assert!(rel(volume(&body), 12_900.0) < 1e-9);
        assert!(body.check(CheckLevel::All).unwrap().is_valid());

        let made = blend(&body, kind, &edges, &[size], &[], Corners::Filled).unwrap();
        assert_eq!(made.dropped, [false; 2]);
        assert_eq!(body.to_record().unwrap().0.shells.len(), 1);
        assert!(rel(volume(&body), 12_900.0 + 20.0 * section) < 1e-9);
        assert!(body.check(CheckLevel::All).unwrap().is_valid());
    }
}

#[test]
fn post_foot_blends_certify_the_limit_with_a_rounded_vertex() {
    all_fixes();
    let (mut record, _) = post_foot(true).to_record().unwrap();
    let exact = [10.0_f64, 0.0, 4.0];
    let displaced = [exact[0].next_down(), exact[1], exact[2]];
    // The application fixture's glued vertex is one ulp off the three exact planes.
    // Only the vertex is changed. The plane supports and line carriers stay exact.
    let vertex = record
        .vertices
        .iter_mut()
        .find(|vertex| vertex.point == exact)
        .unwrap();
    vertex.point = displaced;
    for (kind, fit_kind, section) in [
        (Blend::Chamfer, FitKind::Distance, 25.0 / 2.0),
        (Blend::Round, FitKind::Radius, 25.0 * (1.0 - PI / 4.0)),
    ] {
        let (input, _) = Body::from_record(&record, true).unwrap();
        let (before, _) = input.to_record().unwrap();
        assert!(before
            .vertices
            .iter()
            .any(|vertex| vertex.point == displaced));
        let refused = blend(
            &input,
            kind,
            &post_foot_edges(&input),
            &[6.0],
            &[],
            Corners::Filled,
        );
        assert!(
            matches!(refused, Err(Error::TooLarge { size, kind: reported, at: Some(_), .. })
                if size == 5.0 && reported == fit_kind),
            "{kind:?}: {refused:?}",
        );
        assert_eq!(input.to_record().unwrap().0, before);
        assert!(rel(volume(&input), 12_900.0) < 1e-9);
        assert!(input.check(CheckLevel::All).unwrap().is_valid());

        // The independently reopened input builds exactly at the advertised limit.
        let (body, _) = Body::from_record(&record, true).unwrap();
        let made = blend(
            &body,
            kind,
            &post_foot_edges(&body),
            &[5.0],
            &[],
            Corners::Filled,
        )
        .unwrap();
        assert_eq!(made.dropped, [false; 2]);
        assert!(made.holes.is_empty());
        assert_eq!(body.solid_count().unwrap(), 1);
        assert_eq!(body.to_record().unwrap().0.shells.len(), 1);
        assert!(body.check(CheckLevel::All).unwrap().is_valid());
        assert!(rel(volume(&body), 12_900.0 + 20.0 * section) < 1e-9);
    }
}

#[test]
fn post_foot_blends_do_not_report_a_limit_with_an_obstructed_sweep() {
    use solidcore_ops::{boolean::boolean, BooleanOp};

    all_fixes();
    for (kind, fit_kind, section) in [
        (Blend::Chamfer, FitKind::Distance, 1.0 / 2.0),
        (Blend::Round, FitKind::Radius, 1.0 - PI / 4.0),
    ] {
        for (point, obstructs) in [([10.5, 4.0, 4.0], true), ([20.0, 20.0, 4.0], false)] {
            for asked in [0.25, 4.75, 5.0, 6.0] {
                let boss = make_box(point, [1.0, 2.0, 2.0]).unwrap();
                let body = boolean(post_foot(true), boss, BooleanOp::Union)
                    .unwrap()
                    .unwrap();
                let edges = post_foot_edges(&body);
                let before = body.to_record().unwrap().0;
                let result = blend(&body, kind, &edges, &[asked], &[], Corners::Filled);
                if obstructs && asked >= 4.75 {
                    assert!(
                        result.is_err() && !matches!(result, Err(Error::TooLarge { .. })),
                        "{kind:?} at {point:?}, {asked} mm: {result:?}",
                    );
                } else if asked == 6.0 {
                    assert!(
                        matches!(result, Err(Error::TooLarge { size: 5.0, kind, .. }) if kind == fit_kind),
                        "{kind:?} at {point:?}, {asked} mm: {result:?}",
                    );
                } else {
                    let made = result.unwrap();
                    assert_eq!(made.dropped, [false, false]);
                    assert_eq!(body.to_record().unwrap().0.shells.len(), 1);
                    // This boss stays separate from the two strips and contributes 4 mm³.
                    assert!(rel(volume(&body), 12_904.0 + 20.0 * section * asked * asked) < 1e-9);
                    assert!(body.check(CheckLevel::All).unwrap().is_valid());
                    continue;
                }
                assert_eq!(body.to_record().unwrap().0, before);
                assert!(rel(volume(&body), 12_904.0) < 1e-9);
                assert!(body.check(CheckLevel::All).unwrap().is_valid());
            }
        }
    }
}

#[test]
fn post_foot_blends_do_not_report_a_fit_below_section_resolution() {
    all_fixes();
    for (kind, zone) in [(Blend::Chamfer, 1.0), (Blend::Round, 3.0)] {
        // The plate still has exact supports. Its coarse B-rep zone puts the
        // 5 mm cross section below the preparation's resolution for this kind.
        let body = post_foot(true);
        body.kernel().set_tolerance(zone);
        let picks = post_foot_edges(&body);
        let before = body.to_record().unwrap().0;
        for asked in [6.0, 9.0] {
            let result = blend(&body, kind, &picks, &[asked], &[], Corners::Filled);
            assert!(
                result.is_err() && !matches!(result, Err(Error::TooLarge { .. })),
                "{kind:?} with zone {zone}, {asked} mm: {result:?}",
            );
            assert_eq!(body.to_record().unwrap().0, before);
            assert_eq!(body.kernel().tolerance(), zone);
        }
    }
}

#[test]
fn post_foot_blends_report_the_shortest_support_across_independent_pairs() {
    use solidcore_ops::{boolean::boolean, BooleanOp};

    all_fixes();
    for (kind, fit_kind, area) in [
        (Blend::Chamfer, FitKind::Distance, 9.0 / 2.0),
        (Blend::Round, FitKind::Radius, 9.0 * (1.0 - PI / 4.0)),
    ] {
        let post = make_box([40.0, 0.0, 4.0], [10.0, 10.0, 30.0]).unwrap();
        let aperture = make_box([37.0, 10.0, -1.0], [3.0, 5.0, 6.0]).unwrap();
        let joined = boolean(post_foot(true), post, BooleanOp::Union)
            .unwrap()
            .unwrap();
        let body = boolean(joined, aperture, BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        let counts = body.counts().unwrap();
        let before = 15_840.0;
        let edges = [
            [10.0, 5.0, 4.0],
            [40.0, 5.0, 4.0],
            [5.0, 10.0, 4.0],
            [45.0, 10.0, 4.0],
        ]
        .map(|point| edge_at(&body, point));
        let result = blend(&body, kind, &edges, &[6.0], &[], Corners::Filled);
        let Err(Error::TooLarge {
            size,
            kind: reported,
            at: Some(at),
            ..
        }) = result
        else {
            panic!("{result:?}");
        };
        assert_eq!(size, 3.0);
        assert_eq!(reported, fit_kind);
        assert_eq!(at, [40.0, 5.0, 4.0]);
        assert_eq!(body.counts().unwrap(), counts);
        assert!(rel(volume(&body), before) < 1e-9);

        let made = blend(&body, kind, &edges, &[size], &[], Corners::Filled).unwrap();
        assert_eq!(made.dropped, [false; 4]);
        assert_eq!(body.to_record().unwrap().0.shells.len(), 1);
        assert!(rel(volume(&body), before + 40.0 * area) < 1e-9);
        assert!(body.check(CheckLevel::All).unwrap().is_valid());
    }
}

#[test]
fn post_foot_blends_keep_the_mitre_without_a_through_cut() {
    use solidcore_ops::PointClass;
    all_fixes();
    for (kind, arms, mitre) in [
        (Blend::Chamfer, 10.0, 1.0 / 3.0),
        (Blend::Round, 20.0 * (1.0 - PI / 4.0), 5.0 / 3.0 - PI / 2.0),
    ] {
        let body = post_foot(false);
        blend(
            &body,
            kind,
            &post_foot_edges(&body),
            &[1.0],
            &[],
            Corners::Filled,
        )
        .unwrap();
        // The ordinary mitre adds a square of the profile width at each height. For size one,
        // integrating (1 - z)^2 or (1 - sqrt(1 - (z - 1)^2))^2 gives the mitre volume above.
        let expected = 13_000.0 + arms + mitre;
        let actual = volume(&body);
        assert!(
            rel(actual, expected) < 1e-9,
            "{kind:?}: {actual} != {expected}"
        );
        assert_eq!(
            body.classify_point([10.25, 10.25, 4.25]).unwrap(),
            PointClass::Inside
        );
        assert!(body.check(CheckLevel::All).unwrap().is_valid());
    }
}

#[test]
fn post_foot_blends_stop_at_two_independent_cutout_corners_in_one_call() {
    use solidcore_ops::PointClass;
    use solidcore_ops::{boolean::boolean, BooleanOp};

    all_fixes();
    let chamfer = 5.0_f64;
    let round = 2.0_f64;
    for (kind, size, section) in [
        (Blend::Chamfer, chamfer, chamfer.powi(2) / 2.0),
        (Blend::Round, round, round.powi(2) * (1.0 - PI / 4.0)),
    ] {
        // Reflect the post and aperture across x = 25, leaving both on one plate.
        let post = make_box([40.0, 0.0, 4.0], [10.0, 10.0, 30.0]).unwrap();
        let aperture = make_box([35.0, 10.0, -1.0], [5.0, 5.0, 6.0]).unwrap();
        let joined = boolean(post_foot(true), post, BooleanOp::Union)
            .unwrap()
            .unwrap();
        let body = boolean(joined, aperture, BooleanOp::Subtract)
            .unwrap()
            .unwrap();
        let edges = [
            [10.0, 5.0, 4.0],
            [40.0, 5.0, 4.0],
            [5.0, 10.0, 4.0],
            [45.0, 10.0, 4.0],
        ]
        .map(|point| edge_at(&body, point));
        let made = blend(&body, kind, &edges, &[size], &[], Corners::Filled).unwrap();
        assert_eq!(made.dropped, [false; 4]);
        assert!(made.holes.is_empty());
        assert_eq!(body.solid_count().unwrap(), 1);
        assert_eq!(body.to_record().unwrap().0.shells.len(), 1);
        assert!(body.check(CheckLevel::All).unwrap().is_valid());

        // Four ten-unit arms have disjoint interiors. The two apertures leave the same void.
        let base = 50.0 * 50.0 * 4.0 + 2.0 * (10.0 * 10.0 * 30.0 - 5.0 * 5.0 * 4.0);
        let expected = base + 40.0 * section;
        let actual = volume(&body);
        assert!(
            rel(actual, expected) < 1e-9,
            "{kind:?}: {actual} != {expected}"
        );
        for x in [10.0, 40.0] {
            let _ = vertex_at(&body, [x, 10.0, 4.0 + size]);
        }
        for x in [10.25, 39.75] {
            let point = [x, 10.25, 4.25];
            assert_eq!(
                body.classify_point(point).unwrap(),
                PointClass::Outside,
                "{kind:?} at {point:?}"
            );
        }
    }
}

#[test]
fn three_rounds_meet_at_a_corner_filled_set_back_or_open() {
    all_fixes();
    let edges = |b: &Body| {
        [
            edge_at(b, [15.0, 20.0, 10.0]),
            edge_at(b, [30.0, 10.0, 10.0]),
            edge_at(b, [30.0, 20.0, 5.0]),
        ]
    };
    // the ball's corner
    let b = block();
    blend(&b, Blend::Round, &edges(&b), &[2.0], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let ball = volume(&b);
    // set back 3 at the corner: a different corner, a closed solid
    let b = block();
    let corner = vertex_at(&b, [30.0, 20.0, 10.0]);
    let back = [SetBack {
        vertex: corner,
        distance: 3.0,
    }];
    blend(&b, Blend::Round, &edges(&b), &[2.0], &back, Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    assert!(rel(volume(&b), ball) < 1e-3);
    // opened: the corner's hole, its edges closing one loop, each on a face outside it
    let b = block();
    let corner = vertex_at(&b, [30.0, 20.0, 10.0]);
    let back = [SetBack {
        vertex: corner,
        distance: 3.0,
    }];
    let made = blend(&b, Blend::Round, &edges(&b), &[2.0], &back, Corners::Open).unwrap();
    assert_eq!(made.holes.len(), 1);
    let [faces, edges_n, _] = b.counts().unwrap();
    for h in &made.holes[0].edges {
        assert!(h.edge < edges_n && h.face < faces);
    }
    assert!(made.holes[0].edges.len() >= 3);
}

/// The record order of `b`, and the record index of each edge of the loop through `corners`
/// (x, y) at height `z`, in the loop's order.
fn loop_edges(b: &Body, corners: [[f64; 2]; 4], z: f64) -> (Order, Vec<usize>) {
    let (record, order) = b.to_record().unwrap();
    let at = |i: u32, c: [f64; 2]| {
        let p = record.vertices[i as usize].point;
        (p[0] - c[0]).abs() + (p[1] - c[1]).abs() + (p[2] - z).abs() < 1e-9
    };
    let edges = (0..4)
        .map(|i| {
            let (a, c) = (corners[i], corners[(i + 1) % 4]);
            record
                .edges
                .iter()
                .position(|e| {
                    let [s, t] = e.vertices;
                    (at(s, a) && at(t, c)) || (at(s, c) && at(t, a))
                })
                .unwrap()
        })
        .collect();
    (order, edges)
}

/// The plate of [`blends_turn_round_a_shallow_sharp_corner`]: 2 thick, its top outline 50
/// along x and then 40 turning `kink` degrees (towards the plate for `convex`, away from it
/// else), with the record indices of those two top edges.
fn shallow_corner_plate(kink: f64, convex: bool) -> (Body, [usize; 2]) {
    let t = if convex { -kink } else { kink }.to_radians();
    let (u, n) = ([t.cos(), t.sin()], [t.sin(), -t.cos()]);
    let p = [
        [0.0, 0.0],
        [50.0, 0.0],
        [50.0 + 40.0 * u[0], 40.0 * u[1]],
        [50.0 + 40.0 * u[0] + 30.0 * n[0], 40.0 * u[1] + 30.0 * n[1]],
        [0.0, -30.0],
    ];
    let profile: Vec<Curve> = (0..p.len())
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            Curve::Line {
                start: [a[0], a[1], 0.0],
                end: [b[0], b[1], 0.0],
            }
        })
        .collect();
    let b = extrude(
        &profile,
        [0.0, 0.0, 1.0],
        2.0,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap();
    let edges = [
        edge_at(&b, [25.0, 0.0, 2.0]),
        edge_at(&b, [50.0 + 20.0 * u[0], 20.0 * u[1], 2.0]),
    ];
    (b, edges)
}

/// A plate 2 thick whose top outline runs 50 along x and then 40 at a shallow angle off it,
/// turning `kink` degrees (towards the plate for `convex`, away from it else), its other sides
/// square to those two: the two top edges rounded r 1 or chamfered 1 turn round the sharp
/// upright edge between them, mitred. Each edge loses its section's area A along its length
/// (90 in all), and the mitre on the bisector takes the corner's part off one side and adds it
/// on the other: the section's first moment M about the upright edge times tan(kink / 2), twice,
/// off at the convex corner and on at the concave one. A round's A = (1 - pi/4) r^2 and
/// M = (5/6 - pi/4) r^3; a chamfer's A = d^2 / 2 and M = d^3 / 6. Edges a few degrees off
/// tangent used to be read as continuing each other (within the solvers' 10 degree tangency
/// tolerance) and given one shared section: the two balls stand the radius times the angle
/// apart there, the trim leaked and the blend failed with status 1001.
#[test]
fn blends_turn_round_a_shallow_sharp_corner() {
    all_fixes();
    let round = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    let chamfer = (0.5, 1.0 / 6.0);
    for kink in [0.5, 1.5, 3.0, 6.0] {
        for convex in [true, false] {
            for (kind, (a, m)) in [(Blend::Round, round), (Blend::Chamfer, chamfer)] {
                let (b, edges) = shallow_corner_plate(kink, convex);
                let before = volume(&b);
                blend(&b, kind, &edges, &[1.0], &[], Corners::Filled).unwrap_or_else(|e| {
                    panic!("{kind:?} at a {kink} degree corner (convex {convex}): {e:?}")
                });
                let corner = 2.0 * m * (kink.to_radians() / 2.0).tan();
                let want = 90.0 * a + if convex { -corner } else { corner };
                let off = before - volume(&b);
                assert!(
                    rel(off, want) < 1e-6,
                    "{kind:?} at a {kink} degree corner (convex {convex}): {off} off, want {want}"
                );
            }
        }
    }
}

/// A plate `t` thick whose top face, through the origin, is tilted `tilt` degrees about y,
/// bounded outside by two upright walls that meet in a shallow concave crease: the plane
/// y = -46 for x < 0, and that plane turned `crease` degrees about the upright line through
/// (0, -46) for x > 0. Inside it is bounded by y = -38, and it ends at x = +-30. The top meets the
/// walls at 90 degrees and at 90 - atan(sin(tilt) tan(crease)) degrees. Made as solidcraft-cli
/// makes it: boxes, turned and combined.
fn crease_plate(t: f64, tilt: f64, crease: f64) -> (Body, [usize; 2]) {
    let (d, c, l) = (46.0, 38.0, 30.0);
    let (a, k) = (tilt.to_radians(), crease.to_radians());
    // p' = p A + t: rows the images of x, y, z, then the translation
    let about_z = |th: f64, at: [f64; 3]| {
        let (cs, sn) = (th.cos(), th.sin());
        let m = [[cs, sn, 0.0], [-sn, cs, 0.0], [0.0, 0.0, 1.0]];
        let tr = [0, 1, 2].map(|j| at[j] - (0..3).map(|i| at[i] * m[i][j]).sum::<f64>());
        [
            [m[0][0], m[0][1], m[0][2], 0.0],
            [m[1][0], m[1][1], m[1][2], 0.0],
            [m[2][0], m[2][1], m[2][2], 0.0],
            [tr[0], tr[1], tr[2], 1.0],
        ]
    };
    let outer = make_box([-l, -d, -200.0], [2.0 * l, d - c, 400.0]).unwrap();
    let turned = make_box([0.0, -d, -200.0], [2.0 * l, d - c + 10.0, 400.0]).unwrap();
    turned.transform(&about_z(-k, [0.0, -d, 0.0])).unwrap();
    let outer = solidcore_ops::boolean::boolean(outer, turned, solidcore_ops::BooleanOp::Union)
        .unwrap()
        .unwrap();
    let keep = make_box([-l, -d - 40.0, -200.0], [2.0 * l, d + 40.0 - c, 400.0]).unwrap();
    let outer = solidcore_ops::boolean::boolean(outer, keep, solidcore_ops::BooleanOp::Intersect)
        .unwrap()
        .unwrap();
    let slab = make_box([-271.5, -271.5, -t], [543.0, 543.0, t]).unwrap();
    slab.transform(&[
        [a.cos(), 0.0, -a.sin(), 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [a.sin(), 0.0, a.cos(), 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    let b = solidcore_ops::boolean::boolean(slab, outer, solidcore_ops::BooleanOp::Intersect)
        .unwrap()
        .unwrap();
    let top = |x: f64| -x * a.tan();
    let (xl, xr) = (-l / 2.0, l / 2.0 * k.cos());
    let edges = [
        edge_at(&b, [xl, -d, top(xl)]),
        edge_at(&b, [xr, -d - l / 2.0 * k.sin(), top(xr)]),
    ];
    (b, edges)
}

/// The removed volume of [`crease_plate`]'s two top edges rounded `r`, as the prisms of their
/// sections mitred on the crease's bisector plane: each section the wedge of the top and its
/// wall less the ball's disc, cut off at the plate's bottom, in closed form; each prism measured
/// along its centroid line between its end plane and the bisector plane (exact for a prism cut
/// by planes). The corner's own construction departs from that mitre only within the ball
/// about the corner, so the removed volume lies within that ball's eighth, `pi r^3 / 6`, of it.
/// Values for the radii the test uses (the case's derivation, by hand).
const CREASE_PLATE_MITRE: [(f64, f64); 3] = [(3.0, 148.6327), (3.5, 201.0834), (4.0, 258.7033)];

/// A round wider than the plate's walls are tall (it overflows them, runs on past the plate's
/// bottom and is trimmed there), on two edges meeting at a shallow concave crease between the
/// walls. At the crease's corner the round's piece along the crease line ran past the crease
/// edge's lower end; the piece was the crease edge trimmed to its points' drops, both clamped
/// to that end, so it had no length and the corner's loop did not close ("Corner Geometry
/// Computation Failure"). The piece is now the crease line continued: the rounds build valid
/// at the full check, within the corner ball's eighth of the mitred prisms. Run under the
/// application's fix set (every fix on).
#[test]
fn rounds_overflowing_a_shallow_concave_crease_build() {
    for (r, mitre) in CREASE_PLATE_MITRE {
        rig_fixes();
        let (b, edges) = crease_plate(3.0, 25.0, 15.0);
        let before = volume(&b);
        blend(&b, Blend::Round, &edges, &[r], &[], Corners::Filled)
            .unwrap_or_else(|e| panic!("r {r}: {e:?}"));
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        let off = before - volume(&b);
        let bound = PI * r * r * r / 6.0;
        assert!(
            (off - mitre).abs() <= bound,
            "r {r}: {off} off, mitre {mitre}, bound {bound}"
        );
    }
    all_fixes();
}

/// Asserts that the faces of [`shallow_corner_plate`]`(kink, convex)` rounded r 1 that are not
/// planes are two cylinders, one per round, read from their definitions: radius 1, semi-angle
/// 0, the axis parallel to the round's edge and its origin on the line through the points 1 in
/// from both of the edge's faces (the edge moved 1 into the plate and 1 down), each within
/// 1e-12 (rounding of coordinates up to 90 and unit vectors). One round's surface extended over
/// both, as the cleanup used to make it, stands 1.7e-7 rad off the other axis at 1e-5 degrees.
fn rounds_on_their_cylinders(b: &Body, kink: f64, convex: bool) {
    use solidcore::topology::face::p2 as face;
    let t = if convex { -kink } else { kink }.to_radians();
    let (u, n) = ([t.cos(), t.sin()], [t.sin(), -t.cos()]);
    // (a point, the direction) of each round's axis
    let axes = [
        (Vec3::new(0.0, -1.0, 1.0), Vec3::new(1.0, 0.0, 0.0)),
        (
            Vec3::new(50.0 + n[0], n[1], 1.0),
            Vec3::new(u[0], u[1], 0.0),
        ),
    ];
    let (_k, brep) = kernel_copy(b);
    let mut on = Vec::new();
    for &f in brep.face_list() {
        let s = face::surface(&brep, f).unwrap().unwrap();
        let cone = match s.kind() {
            SurfaceKind::Plane(_) => continue,
            SurfaceKind::Cone(c) => c.canonical(),
            other => panic!("kink {kink} (convex {convex}): a round on {other:?}"),
        };
        assert!(
            (cone.radius - 1.0).abs() <= 1e-12 && cone.semi_angle_deg == 0.0,
            "kink {kink} (convex {convex}): radius {} semi-angle {}",
            cone.radius,
            cone.semi_angle_deg
        );
        let (origin, axis) = (cone.origin.origin(), cone.origin.z_axis());
        let off: Vec<(f64, f64)> = axes
            .iter()
            .map(|(a, d)| (axis.cross(*d).length(), (origin - *a).cross(*d).length()))
            .collect();
        let which = (0..2)
            .find(|&i| off[i].0 <= 1e-12 && off[i].1 <= 1e-12)
            .unwrap_or_else(|| {
                panic!("kink {kink} (convex {convex}): a round {off:?} off both axes")
            });
        on.push(which);
    }
    on.sort_unstable();
    assert_eq!(
        on,
        [0, 1],
        "kink {kink} (convex {convex}): rounds on cylinders {on:?}"
    );
}

/// #15418: the plate of [`blends_turn_round_a_shallow_sharp_corner`] with corners a hundred
/// thousandth to a few ten thousandths of a degree off straight. The two top edges rounded r 1
/// or chamfered 1 build and pass every check. Each round stays its own face on its own
/// closed-form cylinder: radius 1 about the line 1 in from the edge's two faces. The volume
/// taken off is the mitred corner's closed form (the section's area A along the 90 of edge,
/// the corner's first moment M times tan(kink / 2) twice, off at the convex corner and on at
/// the concave one) within the 1e-6 of the sibling test: these corners join on one section,
/// whose volume differs from the mitre's by a multiple of the kink. Rounds at a concave corner
/// 3e-5 to 2e-4 degrees off straight used to come out as one cylinder face, the boolean's
/// cleanup merging the second round into the first's extended surface: its far end stood the
/// first edge's length times the kink off it and failed the check (`SmVertex`'s gap test).
#[test]
fn blends_turn_round_a_corner_a_ten_thousandth_of_a_degree_off_straight() {
    all_fixes();
    let round = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    let chamfer = (0.5, 1.0 / 6.0);
    for kink in [1e-5, 5e-5, 1e-4, 2e-4, 3e-4] {
        for convex in [false, true] {
            for (kind, (a, m)) in [(Blend::Round, round), (Blend::Chamfer, chamfer)] {
                let (b, edges) = shallow_corner_plate(kink, convex);
                let before = volume(&b);
                blend(&b, kind, &edges, &[1.0], &[], Corners::Filled).unwrap_or_else(|e| {
                    panic!("{kind:?} at a {kink} degree corner (convex {convex}): {e:?}")
                });
                assert!(
                    b.check(CheckLevel::All).unwrap().is_valid(),
                    "{kind:?} at a {kink} degree corner (convex {convex})"
                );
                if kind == Blend::Round {
                    rounds_on_their_cylinders(&b, kink, convex);
                }
                let corner = 2.0 * m * (kink.to_radians() / 2.0).tan();
                let want = 90.0 * a + if convex { -corner } else { corner };
                let off = before - volume(&b);
                assert!(
                    rel(off, want) < 1e-6,
                    "{kind:?} at a {kink} degree corner (convex {convex}): {off} off, want {want}"
                );
            }
        }
    }
}

/// #16627: the plate of [`blends_turn_round_a_shallow_sharp_corner`] with corners a thousandth
/// to a hundredth of a degree off straight, both top edges rounded r 1 or chamfered 1. Each
/// result passes every check, each round lies on its own closed-form cylinder, and the volume
/// taken off is the mitred corner's closed form within the sibling tests' 1e-6. Where the two
/// blends' sections at the corner stand 1 x kink apart, a Join puts one vertex there for both,
/// which the check holds within the B-rep's intersection tolerance (2e-5): concave corners
/// 1.5e-3 and 2e-3 degrees off straight (2.6e-5 and 3.5e-5 apart) used to be joined and failed
/// `SmVertex`'s gap test; now they are mitred. The bound is 2e-5 rad, 1.146e-3 degrees:
/// 1.14e-3 still joins, 1.15e-3 is mitred (`wr697_end_and_join.rs` asserts which). Asymmetric
/// chamfers (setbacks 1 and 0.5 either way round) stand at most their larger setback times the
/// kink apart and pass the check on both sides of the bound too.
#[test]
fn blends_join_at_a_near_straight_corner_only_within_the_vertex_check() {
    all_fixes();
    let round = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    let chamfer = (0.5, 1.0 / 6.0);
    for kink in [1e-3, 1.14e-3, 1.15e-3, 1.5e-3, 2e-3] {
        for convex in [false, true] {
            for (top, side) in [(1.0, 0.5), (0.5, 1.0)] {
                let (b, edges) = shallow_corner_plate(kink, convex);
                let along: Vec<usize> = edges
                    .iter()
                    .map(|&e| {
                        let faces = edge_faces_along(&b, e, [0.0, 0.0, 1.0]).unwrap();
                        if faces[0].1 > faces[1].1 {
                            faces[0].0
                        } else {
                            faces[1].0
                        }
                    })
                    .collect();
                chamfer_sides(&b, &edges, &along, top, ChamferSecond::Distance(side))
                    .unwrap_or_else(|e| {
                        panic!("chamfer {top} x {side} at {kink} degrees (convex {convex}): {e:?}")
                    });
                let report = b.check(CheckLevel::All).unwrap();
                assert!(
                    report.is_valid(),
                    "chamfer {top} x {side} at {kink} degrees (convex {convex}): {report}"
                );
            }
        }
    }
    for kink in [1e-3, 1.14e-3, 1.15e-3, 1.5e-3, 2e-3, 3e-3, 5e-3, 1e-2] {
        for convex in [false, true] {
            for (kind, (a, m)) in [(Blend::Round, round), (Blend::Chamfer, chamfer)] {
                let (b, edges) = shallow_corner_plate(kink, convex);
                let before = volume(&b);
                blend(&b, kind, &edges, &[1.0], &[], Corners::Filled).unwrap_or_else(|e| {
                    panic!("{kind:?} at a {kink} degree corner (convex {convex}): {e:?}")
                });
                let report = b.check(CheckLevel::All).unwrap();
                assert!(
                    report.is_valid(),
                    "{kind:?} at a {kink} degree corner (convex {convex}): {report}"
                );
                if kind == Blend::Round {
                    rounds_on_their_cylinders(&b, kink, convex);
                }
                let corner = 2.0 * m * (kink.to_radians() / 2.0).tan();
                let want = 90.0 * a + if convex { -corner } else { corner };
                let off = before - volume(&b);
                assert!(
                    rel(off, want) < 1e-6,
                    "{kind:?} at a {kink} degree corner (convex {convex}): {off} off, want {want}"
                );
            }
        }
    }
}

/// A sketch outline with a near-tangent corner, the whole top rim rounded: a four-sided plate 5
/// thick, its top outline 40 along x, then 40 on at 3.8 degrees off it (two lines meeting at
/// 176.2 degrees), then back to (0, -30) and up to the start. All four top edges rounded r 1.5
/// and r 3, one closed chain through the 3.8 degree corner and the three sharp ones. Every corner
/// is convex and mitred: as in [`blends_turn_round_a_shallow_sharp_corner`], the rim loses
/// A = (1 - pi/4) r^2 along its perimeter P, less 2 M tan(theta / 2) at each corner turning
/// theta, M = (5/6 - pi/4) r^3. The rounds become 4 faces on the plate's 6. Before the shallow
/// corner was mitred, the two rounds meeting at it were given one shared section and the blend
/// failed with status 1001.
#[test]
fn a_rim_round_runs_through_a_near_tangent_corner() {
    all_fixes();
    let t = (-3.8_f64).to_radians();
    let p = [
        [0.0, 0.0],
        [40.0, 0.0],
        [40.0 + 40.0 * t.cos(), 40.0 * t.sin()],
        [0.0, -30.0],
    ];
    let side = |i: usize| {
        let (a, b) = (p[i], p[(i + 1) % 4]);
        [b[0] - a[0], b[1] - a[1]]
    };
    let len = |v: [f64; 2]| (v[0] * v[0] + v[1] * v[1]).sqrt();
    let perimeter: f64 = (0..4).map(|i| len(side(i))).sum();
    // tan of half the turn at each corner, from the sides into and out of it
    let half_turns: f64 = (0..4)
        .map(|i| {
            let (a, b) = (side((i + 3) % 4), side(i));
            let cos = (a[0] * b[0] + a[1] * b[1]) / (len(a) * len(b));
            (cos.acos() / 2.0).tan()
        })
        .sum();
    let area = 0.5
        * (0..4)
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % 4]);
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f64>()
            .abs();
    for r in [1.5, 3.0] {
        let profile: Vec<Curve> = (0..4)
            .map(|i| {
                let (a, b) = (p[i], p[(i + 1) % 4]);
                Curve::Line {
                    start: [a[0], a[1], 0.0],
                    end: [b[0], b[1], 0.0],
                }
            })
            .collect();
        let b = extrude(
            &profile,
            [0.0, 0.0, 1.0],
            5.0,
            0.0,
            DraftCorner::default(),
            true,
        )
        .unwrap();
        let before = b.volume(1e-10).unwrap();
        assert!(rel(before, 5.0 * area) < 1e-12, "r {r}: plate {before}");
        let edges: Vec<usize> = (0..4)
            .map(|i| {
                let (a, c) = (p[i], p[(i + 1) % 4]);
                edge_at(&b, [(a[0] + c[0]) / 2.0, (a[1] + c[1]) / 2.0, 5.0])
            })
            .collect();
        blend(&b, Blend::Round, &edges, &[r], &[], Corners::Filled)
            .unwrap_or_else(|e| panic!("r {r}: {e:?}"));
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        assert_eq!(b.counts().unwrap()[0], 10, "r {r}");
        let want = (1.0 - PI / 4.0) * r * r * perimeter
            - 2.0 * (5.0 / 6.0 - PI / 4.0) * r.powi(3) * half_turns;
        let off = before - b.volume(1e-10).unwrap();
        assert!(rel(off, want) < 1e-9, "r {r}: {off} off, want {want}");
    }
}

/// A 40 x 30 x 10 plate with a 10 x 10 x 5 boss on it (x 15..25, y 10..20), the boss's four
/// base edges rounded r 1: at each upright boss edge two concave rounds turn round the sharp
/// convex edge. Each base edge gains the spandrel A = (1 - pi/4) r^2 along its 10, and each
/// corner the rounds' part past both walls, whose section's first moment about the upright edge
/// is M = (5/6 - pi/4) r^3: mitred, 8 M in all; swept, the section revolved a quarter turn at
/// each, 2 pi M, its face a torus. Unequal rounds are not swept yet. Along a pocket's floor the
/// upright edges are sharp and concave: no ball turns round them, so the swept turn is refused
/// there and the mitre builds, 60 A - 8 M. A swept turn is a round's. A swept turn refused is
/// [`Error::TurnRefused`] with the kernel's reason: whether no radius builds it (the pocket)
/// or the corner is of a kind not built yet (the unequal rounds).
#[test]
fn rounds_turn_round_a_boss_edge_mitred_or_swept() {
    all_fixes();
    let bx = |k: &mut Kernel, lo: [f64; 3], size: [f64; 3]| {
        create_box(k, Vec3::new(lo[0], lo[1], lo[2]), size[0], size[1], size[2])
    };
    let boss = || {
        built(|k| {
            let plate = bx(k, [0.0; 3], [40.0, 30.0, 10.0])?;
            let boss = bx(k, [15.0, 10.0, 10.0], [10.0, 10.0, 5.0])?;
            boolean_union(k, plate, boss)
        })
    };
    let pocket = || {
        built(|k| {
            let plate = bx(k, [0.0; 3], [40.0, 30.0, 10.0])?;
            let cut = bx(k, [10.0, 10.0, 5.0], [20.0, 10.0, 6.0])?;
            boolean_difference(k, plate, cut)
        })
    };
    let base = [[15.0, 10.0], [25.0, 10.0], [25.0, 20.0], [15.0, 20.0]];
    let floor = [[10.0, 10.0], [30.0, 10.0], [30.0, 20.0], [10.0, 20.0]];
    let (a, m) = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    assert_eq!(Turn::default(), Turn::Mitre);
    for (turn, corners, faces) in [(Turn::Mitre, 8.0 * m, 15), (Turn::Swept, 2.0 * PI * m, 19)] {
        let b = boss();
        let (order, edges) = loop_edges(&b, base, 10.0);
        blend_ordered(
            &b,
            &order,
            Blend::Round,
            &edges,
            &[1.0],
            &[],
            Corners::Filled,
            BlendOptions {
                turn,
                ..BlendOptions::default()
            },
        )
        .unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "{turn:?}");
        assert_eq!(b.counts().unwrap()[0], faces, "{turn:?}");
        let v = b.volume(1e-10).unwrap();
        assert!(rel(v, 12500.0 + 40.0 * a + corners) < 1e-9, "{turn:?}: {v}");
        // swept, the four corners' faces are exact horn tori (R = r = 1), as the kernel reads
        // the body back; mitred, there are none. That each blend face answers
        // procedural_blend() is the kernel's definition ledger
        // (crates/solidcore/tests/issue_2489_blend_faces_are_definitions.rs, the boss base with
        // mitred and swept turns): an ops body reads back through its .smb, which keeps the
        // exact rounds' and corners' forms but not their definitions
        let (_, made) = kernel_copy(&b);
        let tori: Vec<(f64, f64)> = made
            .face_list()
            .iter()
            .filter_map(|&f| match made.face(f).unwrap().surface.as_ref()?.kind() {
                SurfaceKind::Torus(t) => Some((t.major_radius(), t.minor_radius())),
                _ => None,
            })
            .collect();
        let want = if turn == Turn::Swept { 4 } else { 0 };
        assert_eq!(tori, vec![(1.0, 1.0); want], "{turn:?}");
    }
    let swept = |b: &Body, order: &Order, kind, edges: &[usize], sizes: &[f64]| {
        blend_ordered(
            b,
            order,
            kind,
            edges,
            sizes,
            &[],
            Corners::Filled,
            BlendOptions {
                turn: Turn::Swept,
                ..BlendOptions::default()
            },
        )
        .err()
    };
    let b = boss();
    let (order, edges) = loop_edges(&b, base, 10.0);
    let unequal = [1.0, 1.5, 1.0, 1.5];
    let r = swept(&b, &order, Blend::Round, &edges, &unequal);
    // not built there yet, with the kernel's reason and the corner's point
    assert!(
        matches!(&r, Some(Error::TurnRefused { room: false, why, at: Some(_) })
            if why.starts_with("a swept turn is built between constant rounds of one radius")),
        "{r:?}"
    );
    let r = swept(&b, &order, Blend::Chamfer, &edges, &[1.0]);
    assert_eq!(r, Some(Error::Argument));
    let p = pocket();
    let (order, edges) = loop_edges(&p, floor, 5.0);
    let before = p.to_record().unwrap().0;
    let r = swept(&p, &order, Blend::Round, &edges, &[1.0]);
    // no radius of the rounds passes the sharp concave edge: the kernel says what would
    assert!(
        matches!(&r, Some(Error::TurnRefused { room: true, why, at: Some(_) })
            if why.contains("round that edge at a radius of at least 1 first")),
        "{r:?}"
    );
    assert_eq!(p.to_record().unwrap().0, before);
    blend_ordered(
        &p,
        &order,
        Blend::Round,
        &edges,
        &[1.0],
        &[],
        Corners::Filled,
        BlendOptions {
            turn: Turn::Mitre,
            ..BlendOptions::default()
        },
    )
    .unwrap();
    let v = p.volume(1e-10).unwrap();
    assert!(rel(v, 11000.0 + 60.0 * a - 8.0 * m) < 1e-9, "{v}");
}

/// [`Turn::Auto`] on the bodies of [`rounds_turn_round_a_boss_edge_mitred_or_swept`]: each turn
/// swept where the kernel builds it, mitred where not, and never refused for its turn. The
/// boss base's concave rounds turn round its convex upright edges, so they are swept, the
/// swept closed form 12500 + 40 A + 2 pi M with four horn tori. The pocket floor's concave
/// rounds meet concave upright edges, where no ball turns, so they are mitred, 11000 + 60 A -
/// 8 M, where every turn swept is refused. Chamfers have no swept turn: Auto mitres them,
/// 12500 + 40 A + 8 M with the chamfer's A = d^2 / 2 and M = d^3 / 6.
#[test]
fn auto_turns_are_swept_where_built_and_mitred_where_not() {
    all_fixes();
    let bx = |k: &mut Kernel, lo: [f64; 3], size: [f64; 3]| {
        create_box(k, Vec3::new(lo[0], lo[1], lo[2]), size[0], size[1], size[2])
    };
    let auto = |b: &Body, order: &Order, kind, edges: &[usize]| {
        blend_ordered(
            b,
            order,
            kind,
            edges,
            &[1.0],
            &[],
            Corners::Filled,
            BlendOptions {
                turn: Turn::Auto,
                ..BlendOptions::default()
            },
        )
    };
    let (a, m) = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    let boss = || {
        built(|k| {
            let plate = bx(k, [0.0; 3], [40.0, 30.0, 10.0])?;
            let boss = bx(k, [15.0, 10.0, 10.0], [10.0, 10.0, 5.0])?;
            boolean_union(k, plate, boss)
        })
    };
    let b = boss();
    let (order, edges) = loop_edges(
        &b,
        [[15.0, 10.0], [25.0, 10.0], [25.0, 20.0], [15.0, 20.0]],
        10.0,
    );
    auto(&b, &order, Blend::Round, &edges).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    assert_eq!(b.counts().unwrap()[0], 19);
    let v = b.volume(1e-10).unwrap();
    assert!(rel(v, 12500.0 + 40.0 * a + 2.0 * PI * m) < 1e-9, "{v}");
    let p = built(|k| {
        let plate = bx(k, [0.0; 3], [40.0, 30.0, 10.0])?;
        let cut = bx(k, [10.0, 10.0, 5.0], [20.0, 10.0, 6.0])?;
        boolean_difference(k, plate, cut)
    });
    let (order, edges) = loop_edges(
        &p,
        [[10.0, 10.0], [30.0, 10.0], [30.0, 20.0], [10.0, 20.0]],
        5.0,
    );
    auto(&p, &order, Blend::Round, &edges).unwrap();
    assert!(p.check(CheckLevel::All).unwrap().is_valid());
    let v = p.volume(1e-10).unwrap();
    assert!(rel(v, 11000.0 + 60.0 * a - 8.0 * m) < 1e-9, "{v}");
    let c = boss();
    let (order, edges) = loop_edges(
        &c,
        [[15.0, 10.0], [25.0, 10.0], [25.0, 20.0], [15.0, 20.0]],
        10.0,
    );
    auto(&c, &order, Blend::Chamfer, &edges).unwrap();
    assert!(c.check(CheckLevel::All).unwrap().is_valid());
    // the chamfers mitred: each base edge gains the chamfer's triangle, A = d^2 / 2, along its
    // 10, and each upright edge the two chamfers' parts past both walls, whose section's first
    // moment about the upright edge is M = d^3 / 6 each (at s from the wall the triangle stands
    // d - s high and runs on s past each end: the integral of (d - s) (10 + 2 s) over 0..d is
    // 10 A + 2 M), so 12500 + 40 A + 8 M. Topology: the boss base's 11 faces, 24 edges and 16
    // vertices; each base edge becomes a chamfer face (15 faces) with its two edges on the wall
    // and on the plate, each base corner a foot on the upright edge and one on the plate (20
    // vertices), joined by the mitre's edge: 24 - 4 + 8 + 4 = 32 edges (20 - 32 + 15 = 3, the
    // plate's top keeping its inner loop round the boss)
    let (a, m) = (0.5, 1.0 / 6.0);
    let v = c.volume(1e-10).unwrap();
    assert!(rel(v, 12500.0 + 40.0 * a + 8.0 * m) < 1e-9, "{v}");
    assert_eq!(c.counts().unwrap(), [15, 32, 20]);
}

/// tangency-labs/solidcraft#16897: an 80 x 60 x 10.5 plate with a 30 x 3 x 65 tab on it
/// (x -15..15, y -1.5..1.5), the tab's four base edges rounded. Past r 1.5 the rounds of the
/// tab's 3 long ends run on past their end face's own extent to meet the long sides' rounds in
/// their mitres (0.41 built them only up to r 1.5).
///
/// The closed form: the plate and tab hold 80 * 60 * 10.5 + 30 * 3 * 65 = 56250. Each base
/// edge gains the spandrel A = (1 - pi/4) r^2 (the square of side r less its quarter disc)
/// along its length, 2 * 30 + 2 * 3 = 66 in all. At each of the four upright tab edges the two
/// mitred rounds add their parts past both walls, 2 M with M = (5/6 - pi/4) r^3, as in
/// [`rounds_turn_round_a_boss_edge_mitred_or_swept`]. The corners at either end of a 3 long
/// edge lie on opposite sides of it (y > 1.5 and y < -1.5), so they never overlap however
/// large r is: V = 56250 + 66 A + 8 M.
#[test]
fn rounds_turn_round_a_thin_tab_wider_than_its_end() {
    all_fixes();
    let base = [[-15.0, -1.5], [15.0, -1.5], [15.0, 1.5], [-15.0, 1.5]];
    for r in [1.5, 2.0, 3.0] {
        let b = built(|k| {
            let plate = create_box(k, Vec3::new(-40.0, -30.0, 0.0), 80.0, 60.0, 10.5)?;
            let tab = create_box(k, Vec3::new(-15.0, -1.5, 10.5), 30.0, 3.0, 65.0)?;
            boolean_union(k, plate, tab)
        });
        let (order, edges) = loop_edges(&b, base, 10.5);
        blend_ordered(
            &b,
            &order,
            Blend::Round,
            &edges,
            &[r],
            &[],
            Corners::Filled,
            BlendOptions::default(),
        )
        .unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "r {r}");
        let (a, m) = ((1.0 - PI / 4.0) * r * r, (5.0 / 6.0 - PI / 4.0) * r * r * r);
        let v = b.volume(1e-10).unwrap();
        assert!(rel(v, 56250.0 + 66.0 * a + 8.0 * m) < 1e-9, "r {r}: {v}");
    }
}

/// [`blend_with`] takes the body's own edge indices and the options [`blend_ordered`] takes:
/// the boss of [`rounds_turn_round_a_boss_edge_mitred_or_swept`], its four base edges picked
/// by point ([`solidcore_ops::pick::edges`]) and rounded r 1, holds 12500 + 40 A + 8 M mitred
/// and 12500 + 40 A + 2 pi M swept (A = 1 - pi/4, M = 5/6 - pi/4), as through the record's
/// order.
#[test]
fn rounds_turn_as_asked_on_the_body_s_own_indices() {
    all_fixes();
    let boss = || {
        built(|k| {
            let plate = create_box(k, Vec3::new(0.0, 0.0, 0.0), 40.0, 30.0, 10.0)?;
            let boss = create_box(k, Vec3::new(15.0, 10.0, 10.0), 10.0, 10.0, 5.0)?;
            boolean_union(k, plate, boss)
        })
    };
    let middles = [
        [20.0, 10.0, 10.0],
        [25.0, 15.0, 10.0],
        [20.0, 20.0, 10.0],
        [15.0, 15.0, 10.0],
    ];
    let (a, m) = (1.0 - PI / 4.0, 5.0 / 6.0 - PI / 4.0);
    for (turn, corners) in [(Turn::Mitre, 8.0 * m), (Turn::Swept, 2.0 * PI * m)] {
        let b = boss();
        let edges = solidcore_ops::pick::edges(&b, &middles, 1e-6)
            .unwrap()
            .unwrap();
        assert_eq!(edges, middles.map(|p| edge_at(&b, p)));
        let options = BlendOptions {
            turn,
            ..BlendOptions::default()
        };
        blend_with(
            &b,
            Blend::Round,
            &edges,
            &[1.0],
            &[],
            Corners::Filled,
            options,
        )
        .unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "{turn:?}");
        let v = b.volume(1e-10).unwrap();
        assert!(rel(v, 12500.0 + 40.0 * a + corners) < 1e-9, "{turn:?}: {v}");
    }
    // the options are checked as blend_ordered checks them
    let b = boss();
    let swept = BlendOptions {
        turn: Turn::Swept,
        ..BlendOptions::default()
    };
    let e = edge_at(&b, middles[0]);
    assert_eq!(
        blend_with(
            &b,
            Blend::Chamfer,
            &[e],
            &[1.0],
            &[],
            Corners::Filled,
            swept
        )
        .err(),
        Some(Error::Argument)
    );
}

/// [`edges_share_wedge`] on solidcore#2250's box (40 x 30 x 20, its top back edge chamfered
/// 5): the chamfer's top edge meets its faces at 135 degrees (the normals 45 degrees apart) and
/// the bottom front edge at 90, so the two share no wedge in either order; the bottom front and
/// top front edges are both square and share one; one edge, or none, shares its own. The
/// measure leaves the body as it was.
#[test]
fn edges_share_a_wedge_where_their_faces_meet_at_one_angle() {
    all_fixes();
    let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
    let back = edge_at(&b, [20.0, 30.0, 20.0]);
    blend(&b, Blend::Chamfer, &[back], &[5.0], &[], Corners::Filled).unwrap();
    let before = b.to_record().unwrap().0;
    let a = edge_at(&b, [20.0, 25.0, 20.0]);
    let bottom = edge_at(&b, [20.0, 0.0, 0.0]);
    let top = edge_at(&b, [20.0, 0.0, 20.0]);
    assert_eq!(edges_share_wedge(&b, &[a, bottom]), Ok(false));
    assert_eq!(edges_share_wedge(&b, &[bottom, a]), Ok(false));
    assert_eq!(edges_share_wedge(&b, &[bottom, top]), Ok(true));
    assert_eq!(edges_share_wedge(&b, &[a]), Ok(true));
    assert_eq!(edges_share_wedge(&b, &[]), Ok(true));
    assert_eq!(edges_share_wedge(&b, &[999]), Err(Error::Argument));
    assert_eq!(b.to_record().unwrap().0, before);
}

/// [`chamfer_sides_facing`] takes each edge's first distance along the face that faces the
/// direction most: on the block's top front edge, 1 along the face facing up and 3 on the
/// other leaves the top its 19 (of 20); facing forward (-y), its 17. Either takes
/// d1 d2 / 2 = 1.5 off along the edge's 30, as [`chamfer_sides`] with that face named.
#[test]
fn chamfers_by_sides_take_the_face_facing_a_direction() {
    all_fixes();
    for (facing, leg) in [([0.0, 0.0, 1.0], 19.0), ([0.0, -1.0, 0.0], 17.0)] {
        let b = block();
        let e = top_front(&b);
        chamfer_sides_facing(&b, &[e], 1.0, ChamferSecond::Distance(3.0), facing).unwrap();
        assert!(rel(volume(&b), ABC - 1.5 * 30.0) < 1e-9, "{facing:?}");
        let (k, made) = kernel_copy(&b);
        let top = made
            .face_list()
            .iter()
            .find_map(|&f| {
                let bx = face_bounding_box(&k, &made, f, true).ok()?;
                ((bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9)
                    .then(|| bx.max().y - bx.min().y)
            })
            .unwrap();
        assert!((top - leg).abs() < 1e-9, "{facing:?}: {top}");
    }
    // the same body as naming the face
    let (named, faced) = (block(), block());
    let e = top_front(&named);
    let faces = edge_faces_along(&named, e, [0.0, 0.0, 1.0]).unwrap();
    let top = if faces[1].1 > faces[0].1 {
        faces[1].0
    } else {
        faces[0].0
    };
    chamfer_sides(&named, &[e], &[top], 1.0, ChamferSecond::Distance(3.0)).unwrap();
    chamfer_sides_facing(
        &faced,
        &[e],
        1.0,
        ChamferSecond::Distance(3.0),
        [0.0, 0.0, 1.0],
    )
    .unwrap();
    assert_eq!(faced.to_record().unwrap().0, named.to_record().unwrap().0);
    // a direction that is not finite or has no length is refused, the body untouched
    let b = block();
    let e = top_front(&b);
    for facing in [[0.0; 3], [f64::NAN, 0.0, 1.0], [0.0, f64::INFINITY, 0.0]] {
        assert_eq!(
            chamfer_sides_facing(&b, &[e], 1.0, ChamferSecond::Distance(3.0), facing),
            Err(Error::Argument),
            "{facing:?}"
        );
    }
    assert!(rel(volume(&b), ABC) < 1e-12);
}

/// The chamfers by sides and the variable rounds take the contract in their `_with` forms: on
/// the block, whose results are valid solids facing out, [`Contract::Proved`] builds the body
/// [`Contract::Closed`] does (the plain call's), each of the six calls.
#[test]
fn chamfers_by_sides_and_variable_rounds_build_alike_under_either_contract() {
    use solidcore_ops::{
        chamfer_sides_facing_with, chamfer_sides_ordered_with, chamfer_sides_with,
        edge_faces_along_ordered, fillet_variable_from_with, fillet_variable_ordered_with,
        fillet_variable_with, Contract,
    };
    type Call<'a> = &'a dyn Fn(&Body, Contract);
    all_fixes();
    let (knots, radii) = ([0.0, 0.0, 1.0, 1.0], [1.0, 2.0]);
    let second = ChamferSecond::Distance(3.0);
    let up = [0.0, 0.0, 1.0];
    let near = [0.0, 0.0, 10.0];
    let record = |f: &dyn Fn(&Body, Contract)| {
        [Contract::Closed, Contract::Proved].map(|c| {
            let b = block();
            f(&b, c);
            b.to_record().unwrap().0
        })
    };
    let top = |b: &Body, e: usize| {
        let f = edge_faces_along(b, e, up).unwrap();
        if f[1].1 > f[0].1 {
            f[1].0
        } else {
            f[0].0
        }
    };
    // the record's order and its index of the top front edge, between (0, 0, 10) and (30, 0, 10)
    let record_top_front = |b: &Body| {
        let (record, order) = b.to_record().unwrap();
        let at = |i: u32, x: f64| {
            let p = record.vertices[i as usize].point;
            (p[0] - x).abs() + p[1].abs() + (p[2] - 10.0).abs() < 1e-9
        };
        let e = record
            .edges
            .iter()
            .position(|e| {
                let [s, t] = e.vertices;
                (at(s, 0.0) && at(t, 30.0)) || (at(s, 30.0) && at(t, 0.0))
            })
            .unwrap();
        (order, e)
    };
    let cases: [Call<'_>; 6] = [
        &|b, c| {
            let e = top_front(b);
            chamfer_sides_with(b, &[e], &[top(b, e)], 1.0, second, c).unwrap();
        },
        &|b, c| {
            let (order, e) = record_top_front(b);
            let f = edge_faces_along_ordered(b, &order, e, up).unwrap();
            let first = if f[1].1 > f[0].1 { f[1].0 } else { f[0].0 };
            chamfer_sides_ordered_with(b, &order, &[e], &[first], 1.0, second, c).unwrap();
        },
        &|b, c| chamfer_sides_facing_with(b, &[top_front(b)], 1.0, second, up, c).unwrap(),
        &|b, c| {
            let v = vertex_at(b, near);
            fillet_variable_with(b, top_front(b), v, 1, &knots, &radii, c).unwrap();
        },
        &|b, c| {
            let (order, e) = record_top_front(b);
            fillet_variable_ordered_with(b, &order, e, near, 1, &knots, &radii, c).unwrap();
        },
        &|b, c| fillet_variable_from_with(b, top_front(b), near, 1, &knots, &radii, c).unwrap(),
    ];
    for (i, f) in cases.iter().enumerate() {
        let [closed, proved] = record(*f);
        assert_eq!(closed, proved, "call {i}");
    }
}

/// [`fillet_variable_from`] runs the law from the end of the edge nearest a point, as
/// [`fillet_variable`] does from that end's vertex: r from 1 to 2 along the block's top front
/// edge from a point near (0, 0, 10) is the body that vertex gives, and from a point nearer
/// the other end the body that end's vertex gives.
#[test]
fn a_variable_round_runs_from_the_end_nearest_a_point() {
    all_fixes();
    let (knots, radii) = ([0.0, 0.0, 1.0, 1.0], [1.0, 2.0]);
    for (near, end) in [
        ([1.0, -2.0, 11.0], [0.0, 0.0, 10.0]),
        ([28.0, 3.0, 10.0], [30.0, 0.0, 10.0]),
    ] {
        let (by_vertex, by_point) = (block(), block());
        let e = top_front(&by_vertex);
        let start = vertex_at(&by_vertex, end);
        fillet_variable(&by_vertex, e, start, 1, &knots, &radii).unwrap();
        fillet_variable_from(&by_point, e, near, 1, &knots, &radii).unwrap();
        assert_eq!(
            by_point.to_record().unwrap().0,
            by_vertex.to_record().unwrap().0,
            "{near:?}"
        );
        assert!(by_point.check(CheckLevel::All).unwrap().is_valid());
    }
    let b = block();
    let e = top_front(&b);
    assert_eq!(
        fillet_variable_from(&b, e, [f64::NAN, 0.0, 0.0], 1, &knots, &radii),
        Err(Error::Argument)
    );
    assert_eq!(
        fillet_variable_from(&b, 99, [0.0; 3], 1, &knots, &radii),
        Err(Error::Argument)
    );
}

/// A variable round of a closed edge, a cylinder's top or bottom rim: a smooth law from 2 at
/// its seam, rising halfway round and back to 2, and 2 all round, builds a valid body whose
/// volume lies between the constant rounds' of 2 and 4 (the same as 2's for 2 all round). The
/// station closing the loop used to take t at the rim's start, not its end, and the round
/// failed with status 1001 (#18542).
#[test]
fn a_variable_round_of_a_closed_rim_builds() {
    all_fixes();
    let cylinder = || make_cylinder([0.0; 3], 20.0, 60.0).unwrap();
    let constant = |e: usize, r: f64| {
        let b = cylinder();
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
        volume(&b)
    };
    for e in [1, 2] {
        let (v2, v4) = (constant(e, 2.0), constant(e, 4.0));
        for (knots, radii) in [
            (
                &[0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0][..],
                &[2.0, 2.0, 4.0, 2.0, 2.0][..],
            ),
            (&[0.0, 0.0, 1.0, 1.0][..], &[2.0, 2.0][..]),
        ] {
            let b = cylinder();
            let degree = knots.len() - radii.len() - 1;
            fillet_variable_from(&b, e, [20.0, 0.0, 60.0], degree, knots, radii).unwrap();
            assert!(
                b.check(CheckLevel::All).unwrap().is_valid(),
                "edge {e}, {radii:?}"
            );
            let v = volume(&b);
            if radii.len() == 2 {
                assert!(rel(v, v2) < 1e-6, "edge {e}: {v} against {v2}");
            } else {
                assert!(v4 < v && v < v2, "edge {e}: {v} not between {v4} and {v2}");
            }
        }
    }
}

#[test]
fn chamfers_by_sides_take_the_face_asked() {
    all_fixes();
    // 1 back along the top, 3 down the front: d1 d2 / 2 off per unit length either way round,
    // and the top's leg is 1 when the top is asked first
    for up in [true, false] {
        let b = block();
        let e = top_front(&b);
        let faces = edge_faces_along(&b, e, [0.0, 0.0, 1.0]).unwrap();
        let top = if faces[0].1 > faces[1].1 {
            faces[0].0
        } else {
            faces[1].0
        };
        let front = if faces[0].1 > faces[1].1 {
            faces[1].0
        } else {
            faces[0].0
        };
        let first = if up { top } else { front };
        chamfer_sides(&b, &[e], &[first], 1.0, ChamferSecond::Distance(3.0)).unwrap();
        assert!(rel(volume(&b), ABC - 1.5 * 30.0) < 1e-9);
        // the top keeps its leg: (30 x (20 - 1)) when asked first, else 20 - 3
        let leg = if up { 19.0 } else { 17.0 };
        let (k, made) = kernel_copy(&b);
        let top_area = made
            .face_list()
            .iter()
            .find_map(|&f| {
                let bx = face_bounding_box(&k, &made, f, true).ok()?;
                ((bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9)
                    .then(|| bx.max().y - bx.min().y)
            })
            .unwrap();
        assert!((top_area - leg).abs() < 1e-9, "{top_area}");
    }
    // a 45 degree chamfer 2 back along the top is the equal one
    let b = block();
    let e = top_front(&b);
    let top = edge_faces_along(&b, e, [0.0, 0.0, 1.0]).unwrap();
    let top = if top[0].1 > top[1].1 {
        top[0].0
    } else {
        top[1].0
    };
    chamfer_sides(&b, &[e], &[top], 2.0, ChamferSecond::Angle(PI / 4.0)).unwrap();
    assert!(rel(volume(&b), ABC - 2.0 * 30.0) < 1e-9);
}

#[test]
fn a_variable_round_runs_from_its_start() {
    all_fixes();
    // r from 1 to 2 along the top front edge: between the constant rounds of 1 and 2
    let b = block();
    let e = top_front(&b);
    let start = vertex_at(&b, [0.0, 0.0, 10.0]);
    fillet_variable(&b, e, start, 1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0]).unwrap();
    let v = volume(&b);
    let off = |r: f64| (1.0 - PI / 4.0) * r * r * 30.0;
    assert!(v < ABC - off(1.0) && v > ABC - off(2.0), "{v}");
    assert!(b.check(CheckLevel::All).unwrap().is_valid());

    // a law zero inside, one that starts off the edge
    let b = block();
    let e = top_front(&b);
    let start = vertex_at(&b, [0.0, 0.0, 10.0]);
    let pinched = fillet_variable(
        &b,
        e,
        start,
        1,
        &[0.0, 0.0, 0.5, 1.0, 1.0],
        &[1.0, 0.0, 1.0],
    );
    assert!(matches!(pinched, Err(Error::Unsupported(_))), "{pinched:?}");
    let away = vertex_at(&b, [30.0, 20.0, 0.0]);
    let off_edge = fillet_variable(&b, e, away, 1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0]);
    assert_eq!(off_edge, Err(Error::Argument));
    // refused: the body as it came in
    assert!(rel(volume(&b), ABC) < 1e-12);
}

/// #14405: a variable round's definition net is certified on wide pieces. Over a piece of
/// the round the touch circle's rows are bounded by Taylor models in s as well as by
/// interval series, which take the rails and the ball's centre (they move together) as
/// independent ranges: with those alone the circle proved only on pieces some hundredths of
/// the round long, and this round took 113 span certificates and 8,592 enclosures (about
/// 10 s at opt 3), its net 23 inner knots along the round; now 3 and 66 (0.6 s), and 1.
/// The net is the balls' envelope within its certified bound wherever sampled (its distance
/// from the law's balls, measured without the certificate: 1.6e-14, the bound 3.2e-6), and
/// the body is sound.
#[test]
fn a_variable_round_s_net_is_certified_on_wide_pieces() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
    let (from, middle) = ([10.0, 0.0, 30.0], [10.0, 10.0, 30.0]);
    let e = edge_at(&b, middle);
    let start = vertex_at(&b, from);
    fillet_variable(&b, e, start, 1, &[0.0, 0.0, 1.0, 1.0], &[2.0, 5.0]).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let (_, made) = kernel_copy(&b);
    // the ball of radius 2 at the start touches the faces x = 10 and z = 30
    let balls = LawPieces::new(from, middle, [[10.0, 0.0, 28.0], [8.0, 0.0, 30.0]], 2.0);
    let mut rounds = 0;
    for &f in made.face_list() {
        let s = made.face(f).unwrap().surface.as_ref().unwrap();
        let Some(round @ BlendRef::Procedural(pb)) = s.procedural_blend() else {
            continue;
        };
        rounds += 1;
        let net = pb.net();
        let d = net.natural_uv_domain();
        let (_, _, swapped) = pb.definition_map();
        let nurb = net.gw_nurb().unwrap();
        let (knots, lo, hi) = if swapped {
            (nurb.v_knots(), d.min().y, d.max().y)
        } else {
            (nurb.u_knots(), d.min().x, d.max().x)
        };
        let mut inner: Vec<f64> = knots
            .iter()
            .copied()
            .filter(|&x| x > lo && x < hi)
            .collect();
        inner.dedup();
        let bound = round.bound();
        let mut worst = 0.0_f64;
        for i in 0..=80 {
            for j in 0..=8 {
                let (a, c) = (f64::from(i) / 80.0, f64::from(j) / 8.0);
                let uv = Point2::new(
                    d.min().x + a * (d.max().x - d.min().x),
                    d.min().y + c * (d.max().y - d.min().y),
                );
                let p = net.evaluate_point(uv).unwrap();
                worst = worst.max(balls.off(p, |y| 2.0 + 3.0 * y));
            }
        }
        eprintln!(
            "{} inner knots, bound {bound:e}, off the balls {worst:e}",
            inner.len()
        );
        assert!(inner.len() <= 2, "{} inner knots", inner.len());
        assert!(worst <= bound, "{worst} over {bound}");
    }
    assert_eq!(rounds, 1);
}

/// tangency-labs/solidcraft#17640: a kernel certifies a record's procedural blend once. Every
/// boolean in an application gives back a record that the next operation reads again, and each
/// read gave every blend its definition back and certified its net again (6 s a boolean on a
/// part of 60 such rounds). Read on one kernel twice, the variable round's record certifies its
/// blend the first time only, and builds the same body both times; with the memo off
/// (`memoize_curve_caches`, as `curve-memo-off` builds every kernel) each read certifies it.
#[test]
fn a_records_procedural_blend_is_certified_once_on_a_kernel() {
    all_fixes();
    let b = make_box([0.0; 3], [10.0, 20.0, 30.0]).unwrap();
    let e = edge_at(&b, [10.0, 10.0, 30.0]);
    let start = vertex_at(&b, [10.0, 0.0, 30.0]);
    fillet_variable(&b, e, start, 1, &[0.0, 0.0, 1.0, 1.0], &[2.0, 5.0]).unwrap();
    let (record, _) = b.to_record().unwrap();
    let blends = record
        .faces
        .iter()
        .filter(|f| matches!(f.surface, solidcore_record::brep::Surface::Blend { .. }))
        .count() as u64;
    assert!(blends > 0, "the variable round kept no definition");
    for memo in [true, false] {
        let mut k = Kernel {
            fixes: solidcore::kernel::UpstreamFixes::all(),
            memoize_curve_caches: memo,
            ..Kernel::default()
        };
        let read = |k: &mut Kernel| {
            let before = k.work_counters.blends_recertified();
            let (mut made, _) = solidcore_convert::brep::to_kernel(k, &record, true).unwrap();
            let (again, _) = solidcore_convert::brep::to_record(k, &mut made).unwrap();
            (k.work_counters.blends_recertified() - before, again)
        };
        let (first, once) = read(&mut k);
        let (second, twice) = read(&mut k);
        assert_eq!(
            first, blends,
            "memo {memo}: the first read certifies each blend"
        );
        assert_eq!(
            second,
            if memo { 0 } else { blends },
            "memo {memo}: the second read"
        );
        assert_eq!(
            once, twice,
            "memo {memo}: the two reads build different bodies"
        );
    }
    // A hit needs every input of the certificate to match. A net with one control point moved,
    // or the same record on a kernel of other tolerances, is certified again (whether or not it
    // certifies), however often the record was read before.
    let mut moved = record.clone();
    let face = moved
        .faces
        .iter_mut()
        .find(|f| matches!(f.surface, solidcore_record::brep::Surface::Blend { .. }))
        .unwrap();
    match face.nurbs.as_mut() {
        Some(solidcore_record::brep::Surface::Spline { points, .. }) => points[1][1][0] += 1e-9,
        Some(solidcore_record::brep::Surface::RationalSpline { homogeneous, .. }) => {
            homogeneous[1][1][0] += 1e-9;
        }
        other => panic!("the blend's net is {other:?}"),
    }
    let mut k = Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        memoize_curve_caches: true,
        ..Kernel::default()
    };
    let certified = |k: &mut Kernel, r: &Record| {
        let before = k.work_counters.blends_recertified();
        let _ = solidcore_convert::brep::to_kernel(k, r, true);
        k.work_counters.blends_recertified() - before
    };
    assert_eq!(certified(&mut k, &record), blends);
    assert_eq!(certified(&mut k, &record), 0);
    assert!(
        certified(&mut k, &moved) >= 1,
        "a moved net was taken as certified"
    );
    k.tolerances.model_size_estimate *= 2.0;
    assert_eq!(
        certified(&mut k, &record),
        blends,
        "other tolerances were taken as the same"
    );
}

/// A law with a corner rounds its edge as the union of its balls (`docs/generic-blend.md` §3.2,
/// "Corners in a radius law"): at a peak the two sides' rounds cross and end on their crossing,
/// at a dip the corner ball's zone lies between them. Each removes its balls' union, integrated
/// numerically (SolidCraft's `variable_radius_with_a_corner_through_a_middle_point` for the
/// peak, the fork's capi cases for the others, as `wr685_law_corners` holds them), to that
/// test's 1e-4 relative; each keeps the law's radius at its start and is one sound body.
#[test]
fn a_variable_round_with_a_corner_in_its_law_is_its_balls_union() {
    // box size, the edge's middle, its start, the start's rail ends, knots, radii, removed, faces
    type Case<'a> = (
        [f64; 3],
        [f64; 3],
        [f64; 3],
        [[f64; 3]; 2],
        &'a [f64],
        &'a [f64],
        f64,
        usize,
    );
    all_fixes();
    let peak = [0.0, 0.0, 0.5, 1.0, 1.0];
    let dip_then_peak = [0.0, 0.0, 0.25, 0.75, 1.0, 1.0];
    let cases: [Case<'_>; 4] = [
        // 2 -> 4 -> 2 on the 10 x 20 x 30 box's top edge x = 10, z = 30, from y = 0
        (
            [10.0, 20.0, 30.0],
            [10.0, 10.0, 30.0],
            [10.0, 0.0, 30.0],
            [[8.0, 0.0, 30.0], [10.0, 0.0, 28.0]],
            &peak,
            &[2.0, 4.0, 2.0],
            39.541034,
            8,
        ),
        // 4 -> 2 -> 4 on the 20 cube's edge y = z = 20, from x = 0
        (
            [20.0; 3],
            [10.0, 20.0, 20.0],
            [0.0, 20.0, 20.0],
            [[0.0, 16.0, 20.0], [0.0, 20.0, 16.0]],
            &peak,
            &[4.0, 2.0, 4.0],
            39.542073,
            9,
        ),
        // 4, 2, 4, 2 at 0, 0.25, 0.75, 1 on the same edge, from either end (mirror images)
        (
            [20.0; 3],
            [10.0, 20.0, 20.0],
            [0.0, 20.0, 20.0],
            [[0.0, 16.0, 20.0], [0.0, 20.0, 16.0]],
            &dip_then_peak,
            &[4.0, 2.0, 4.0, 2.0],
            38.818277,
            10,
        ),
        (
            [20.0; 3],
            [10.0, 20.0, 20.0],
            [20.0, 20.0, 20.0],
            [[20.0, 16.0, 20.0], [20.0, 20.0, 16.0]],
            &dip_then_peak,
            &[4.0, 2.0, 4.0, 2.0],
            38.818277,
            10,
        ),
    ];
    for (size, middle, from, rails, knots, radii, want, faces) in cases {
        let b = make_box([0.0; 3], size).unwrap();
        let e = edge_at(&b, middle);
        let start = vertex_at(&b, from);
        fillet_variable(&b, e, start, 1, knots, radii).unwrap();
        let removed = size[0] * size[1] * size[2] - volume(&b);
        eprintln!("{radii:?} from {from:?}: removed {removed}, want {want}");
        assert!(
            (removed - want).abs() <= 1e-4 * want,
            "{radii:?} from {from:?}: removed {removed}, want {want}"
        );
        assert_eq!(b.counts().unwrap()[0], faces, "{radii:?} from {from:?}");
        // the round's end at its start is a section of the law's first radius
        for p in rails {
            vertex_at(&b, p);
        }
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
        // every face the split makes is the round's definition, not a trace of it: each piece
        // between the law's corners a rolling-ball round of its own piece of the law, and each
        // dip's zone the corner ball's patch
        let (k, made) = kernel_copy(&b);
        let corners: Vec<f64> = knots
            .windows(2)
            .filter(|w| w[0] != w[1])
            .map(|w| w[0])
            .collect();
        let corners = [corners.as_slice(), &[1.0]].concat();
        let dips = (1..radii.len() - 1)
            .filter(|&i| radii[i] < radii[i - 1] && radii[i] < radii[i + 1])
            .count();
        let piece_round = LawPieces::new(from, middle, rails, radii[0]);
        let (mut pieces, mut zones) = (Vec::new(), 0);
        for &f in made.face_list() {
            let s = made.face(f).unwrap().surface.as_ref().unwrap();
            match s.procedural_blend() {
                _ if matches!(s.kind(), SurfaceKind::Plane(_)) => {}
                Some(BlendRef::Procedural(pb)) => {
                    // the piece whose law every point of the definition rolls
                    let law = (0..radii.len() - 1).find(|&i| {
                        let r = |y: f64| {
                            let c = (y - corners[i]) / (corners[i + 1] - corners[i]);
                            radii[i] + c * (radii[i + 1] - radii[i])
                        };
                        definition_points(&k, pb)
                            .iter()
                            .all(|&p| piece_round.off(p, r) < 1e-6)
                    });
                    pieces.push(law);
                }
                // (the copy keeps the zone's ball patch, not the corner definition beside it)
                None if surface_kind_name(s) == "ball-patch" => zones += 1,
                _ => panic!("{radii:?} from {from:?}: {:?}", surface_kind_name(s)),
            }
        }
        pieces.sort_unstable();
        let want: Vec<Option<usize>> = (0..radii.len() - 1).map(Some).collect();
        assert_eq!(pieces, want, "{radii:?} from {from:?}");
        assert_eq!(zones, dips, "{radii:?} from {from:?}");
    }
}

/// A law with a corner rounds the edges of a slot and a step as it rounds a block's outside
/// edge (#21179): the trace lands on the law's corner, and takes the through point there
/// whatever its tangent's sign, which is the surfaces' and not the trace's. Each round adds
/// or removes its balls' union along the edge, the faces square to it going on past its ends
/// (the volumes the kernel's `wr685_law_corners` checks against the union's integral), and is
/// a valid body: a peak, a dip, and a corner off the middle.
#[test]
fn a_variable_round_with_a_corner_in_its_law_rounds_a_slots_and_a_steps_edge() {
    use solidcore_ops::{boolean::boolean, BooleanOp};
    all_fixes();
    let cut = |origin, size| {
        let block = make_box([-30.0, -9.5, 0.0], [30.0, 19.0, 30.0]).unwrap();
        let tool = make_box(origin, size).unwrap();
        boolean(block, tool, BooleanOp::Subtract).unwrap().unwrap()
    };
    let slot = cut([-5.0, -10.0, 12.0], [12.0, 20.0, 6.0]);
    let step = cut([-5.0, -10.0, 20.0], [12.0, 20.0, 20.0]);
    // the law's corner and its radii
    let laws = [
        (0.5, [1.0, 2.0, 1.0]),
        (0.5, [1.0, 0.5, 1.0]),
        (0.3, [1.0, 2.0, 1.0]),
    ];
    // the body, a point of the edge, its start, and what each law's round adds
    let (l6, l19, l10) = (
        [2.900200, 0.744330, 2.883669],
        [9.479435, 2.376334, 9.472978],
        [4.942629, 1.247737, 4.931130],
    );
    let edges = [
        (
            &slot,
            [-5.0, -9.5, 15.0],
            [-5.0, -9.5, 12.0],
            l6.map(|v| -v),
        ),
        (&slot, [-5.0, 0.0, 12.0], [-5.0, -9.5, 12.0], l19),
        (&step, [-5.0, 0.0, 20.0], [-5.0, -9.5, 20.0], l19),
        (
            &step,
            [-5.0, -9.5, 25.0],
            [-5.0, -9.5, 20.0],
            l10.map(|v| -v),
        ),
    ];
    for (body, at, start, added) in edges {
        for ((corner, radii), want) in laws.into_iter().zip(added) {
            let b = body.copy().unwrap();
            let before = volume(&b);
            let knots = [0.0, 0.0, corner, 1.0, 1.0];
            fillet_variable_from(&b, edge_at(&b, at), start, 1, &knots, &radii).unwrap();
            let got = volume(&b) - before;
            assert!(
                (got - want).abs() <= 1e-4 * want.abs(),
                "{at:?} {radii:?} at {corner}: {got}, want {want}"
            );
            assert!(
                b.check(CheckLevel::All).unwrap().is_valid(),
                "{at:?} {radii:?} at {corner}"
            );
        }
    }
}

/// A round of a linear radius law along a box's straight edge, between its two faces at a
/// right angle: the ball of radius r(y) at the edge's fraction y lies r(y) in from each face.
struct LawPieces {
    from: Vec3,
    along: Vec3,
    inward: Vec3,
}

impl LawPieces {
    /// The edge from `from` through its middle `middle`, its start's rail ends `rails` at
    /// radius `r0`.
    fn new(from: [f64; 3], middle: [f64; 3], rails: [[f64; 3]; 2], r0: f64) -> Self {
        let v = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
        let from = v(from);
        let [a, b] = rails.map(|p| (v(p) - from) / r0);
        Self {
            from,
            along: (v(middle) - from) * 2.0,
            inward: a + b,
        }
    }

    /// How far `p` is off the envelope of the balls of radius `r(y)` centred at
    /// C(y) = from + y along + r(y) inward: h(y) = |p − C(y)|² − r(y)² is at least 0 for
    /// every y exactly where p is outside every ball, and 0 where it touches one. For a linear
    /// `r` it is a quadratic in y whose leading coefficient is |along|² + r′² > 0 (the edge is
    /// square to its faces' normals), so a ternary search over the edge and a length past each
    /// end finds its least value, which is 2 r (|p − C| − r) to first order: halved over r, a
    /// distance.
    fn off(&self, p: Vec3, r: impl Fn(f64) -> f64) -> f64 {
        let h = |y: f64| {
            let d = p - (self.from + self.along * y + self.inward * r(y));
            d.x * d.x + d.y * d.y + d.z * d.z - r(y) * r(y)
        };
        let (mut lo, mut hi) = (-1.0, 2.0);
        for _ in 0..200 {
            let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if h(a) < h(b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        let y = 0.5 * (lo + hi);
        (h(y) / (2.0 * r(y))).abs()
    }
}

/// Points of `pb`'s definition, on a 5 x 5 grid of its net.
fn definition_points(k: &Kernel, pb: &ProceduralBlend) -> Vec<Vec3> {
    let d = pb.net().natural_uv_domain();
    let (lo, hi) = (d.min(), d.max());
    let mut out = Vec::new();
    for i in 0..5 {
        for j in 0..5 {
            let (a, b) = (f64::from(i) / 4.0, f64::from(j) / 4.0);
            let uv = Point2::new(lo.x + a * (hi.x - lo.x), lo.y + b * (hi.y - lo.y));
            let st = pb.definition_parameters(uv);
            out.push(pb.evaluate_definition(k, st.x, st.y, 0, 0).unwrap()[0][0]);
        }
    }
    out
}

/// Select the cube's edge along x at y = z = 20, with its record Order.
fn variable_ordered_edge(b: &Body) -> (Order, usize) {
    let (record, order) = b.to_record().unwrap();
    let edge = record
        .edges
        .iter()
        .position(|e| {
            e.vertices.iter().all(|&v| {
                let p = record.vertices[usize::try_from(v).unwrap()].point;
                p[1] == 20.0 && p[2] == 20.0
            })
        })
        .unwrap();
    (order, edge)
}

#[test]
fn a_record_ordered_variable_round_keeps_its_selected_edge() {
    all_fixes();
    for permuted in [false, true] {
        let b = make_box([0.0; 3], [20.0; 3]).unwrap();
        let (mut order, edge) = variable_ordered_edge(&b);
        let selected = order.edges[edge];
        if permuted {
            order.edges.rotate_left(5);
        }
        let edge = order.edges.iter().position(|&e| e == selected).unwrap();
        solidcore_ops::fillet_variable_ordered(
            &b,
            &order,
            edge,
            [-10.0, 20.0, 20.0],
            1,
            &[0.0, 0.0, 1.0, 1.0],
            &[2.0, 2.0],
        )
        .unwrap();
        let want = 8000.0 - (1.0 - PI / 4.0) * 4.0 * 20.0;
        assert!((b.volume(1e-9).unwrap() - want).abs() <= want * 1e-9);
        for point in [
            [0.0, 18.0, 20.0],
            [0.0, 20.0, 18.0],
            [20.0, 18.0, 20.0],
            [20.0, 20.0, 18.0],
        ] {
            vertex_at(&b, point);
        }
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
    }
}

/// Reversing this law leaves its total volume the same, so its start's radius-4 rail
/// vertices and its ten faces, not volume alone, prove which end receives the law.
#[test]
fn a_record_ordered_variable_round_keeps_the_near_end_and_tie_rule() {
    all_fixes();
    for point in [[-10.0, 20.0, 20.0], [30.0, 20.0, 20.0], [10.0, 20.0, 20.0]] {
        let b = make_box([0.0; 3], [20.0; 3]).unwrap();
        let (record, _) = b.to_record().unwrap();
        let (mut order, edge) = variable_ordered_edge(&b);
        let selected = order.edges[edge];
        let solidcore_record::brep::Curve::Line { start, .. } = record.edges[edge].curve else {
            panic!("the selected edge is a line");
        };
        let from_x = if point[0] < 0.0 {
            0.0
        } else if point[0] > 20.0 {
            20.0
        } else {
            start[0]
        };
        order.edges.reverse();
        let edge = order.edges.iter().position(|&e| e == selected).unwrap();
        solidcore_ops::fillet_variable_ordered(
            &b,
            &order,
            edge,
            point,
            1,
            &[0.0, 0.0, 0.25, 0.75, 1.0, 1.0],
            &[4.0, 2.0, 4.0, 2.0],
        )
        .unwrap();
        assert_eq!(b.counts().unwrap()[0], 10, "start {point:?}");
        vertex_at(&b, [from_x, 16.0, 20.0]);
        vertex_at(&b, [from_x, 20.0, 16.0]);
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
    }
}

#[test]
fn an_ordered_variable_round_rejects_bad_selection_without_editing() {
    all_fixes();
    let b = make_box([0.0; 3], [20.0; 3]).unwrap();
    let (before, _) = b.to_record().unwrap();
    let (order, edge) = variable_ordered_edge(&b);
    let law = |order: &Order, edge: usize, start| {
        solidcore_ops::fillet_variable_ordered(
            &b,
            order,
            edge,
            start,
            1,
            &[0.0, 0.0, 1.0, 1.0],
            &[2.0, 2.0],
        )
    };
    assert_eq!(
        law(&order, usize::MAX, [0.0, 20.0, 20.0]),
        Err(Error::Argument)
    );
    let mut missing = order.clone();
    missing.edges[edge] = EdgeId::from_index(1_000_000);
    assert_eq!(law(&missing, edge, [0.0, 20.0, 20.0]), Err(Error::Argument));
    assert_eq!(
        law(&order, edge, [f64::NAN, 20.0, 20.0]),
        Err(Error::Argument)
    );
    assert_eq!(b.to_record().unwrap().0, before);
}

#[test]
fn arguments_are_checked_before_the_kernel() {
    all_fixes();
    let b = block();
    let e = top_front(&b);
    let round = |edges: &[usize], sizes: &[f64]| {
        blend(&b, Blend::Round, edges, sizes, &[], Corners::Filled)
    };
    assert_eq!(round(&[], &[1.0]), Err(Error::Argument));
    assert_eq!(round(&[e, e], &[1.0]), Err(Error::Argument));
    assert_eq!(round(&[999], &[1.0]), Err(Error::Argument));
    assert_eq!(round(&[e], &[0.0]), Err(Error::Argument));
    assert_eq!(round(&[e], &[f64::NAN]), Err(Error::Argument));
    assert_eq!(round(&[e], &[2e6]), Err(Error::Argument));
    assert_eq!(round(&[e], &[1.0, 2.0]), Err(Error::Argument));
    let corner = vertex_at(&b, [30.0, 0.0, 10.0]);
    let back = [SetBack {
        vertex: corner,
        distance: 1.0,
    }];
    assert!(matches!(
        blend(&b, Blend::Chamfer, &[e], &[1.0], &back, Corners::Filled),
        Err(Error::Unsupported(_))
    ));
    // a setback at a vertex the kernel does not set back (the end of one round), however
    // long: that corner is refused (#3145: the length is the kernel's, at a real corner)
    let long = [SetBack {
        vertex: corner,
        distance: 50.0,
    }];
    assert_eq!(
        blend(&b, Blend::Round, &[e], &[1.0], &long, Corners::Filled),
        Err(Error::CornerRefused { corner: 0 })
    );
    assert_eq!(
        chamfer_sides(&b, &[e], &[], 1.0, ChamferSecond::Distance(1.0)),
        Err(Error::Argument)
    );
    assert_eq!(
        chamfer_sides(&b, &[e], &[0], 1.0, ChamferSecond::Angle(PI)),
        Err(Error::Argument)
    );
    assert_eq!(
        edge_faces_along(&b, e, [f64::NAN, 0.0, 0.0]),
        Err(Error::Argument)
    );
    assert_eq!(
        edge_faces_along(&b, 999, [0.0, 0.0, 1.0]),
        Err(Error::Argument)
    );
    // nothing changed the body
    assert!(rel(volume(&b), ABC) < 1e-12);
}

/// A distance and an angle whose chamfer runs out past the other face: the refusal names the
/// angle and the bound below which one meets that face, pi less the wedge between the faces.
#[test]
fn a_chamfer_angle_past_the_other_face_is_refused_naming_its_bound() {
    all_fixes();
    let refused = |b: &Body, e: usize, angle: f64, largest: f64| {
        let first = edge_faces_along(b, e, [0.0, 0.0, -1.0]).unwrap()[0].0;
        let before = volume(b);
        match chamfer_sides(b, &[e], &[first], 2.0, ChamferSecond::Angle(angle)) {
            Err(Error::ChamferAngleTooLarge {
                edge,
                angle: asked,
                largest: bound,
            }) => {
                assert_eq!(edge, e);
                assert_eq!(asked, angle);
                assert!(rel(bound, largest) < 1e-12, "{bound} against {largest}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(volume(b), before, "a refusal leaves the body as it came in");
    };
    // a right angle between the faces: the top rim of a cylinder at 90.573 degrees
    let b = make_cylinder([0.0; 3], 10.0, 20.0).unwrap();
    refused(
        &b,
        edge_at(&b, [0.0, 10.0, 20.0]),
        1.580797063408824,
        PI / 2.0,
    );
    // and a straight edge, exactly at the bound as well as past it
    let b = block();
    let e = top_front(&b);
    refused(&b, e, PI / 2.0, PI / 2.0);
    refused(&b, e, 0.75 * PI, PI / 2.0);
    // 45 degrees between a frustum's base and its side: room for 135
    let b = make_cone([0.0; 3], 10.0, 5.0, 5.0).unwrap();
    refused(&b, edge_at(&b, [0.0, 10.0, 0.0]), 2.5, 0.75 * PI);
}

/// The index of the edge of `b` nearest `p` (a case's pick lies off its edge).
fn nearest_edge(b: &Body, p: [f64; 3]) -> usize {
    let (k, brep) = kernel_copy(b);
    let q = Vec3::new(p[0], p[1], p[2]);
    let distance = |&e: &EdgeId| {
        edge_closest_point(&k, &brep, e, q)
            .unwrap()
            .closest
            .distance
    };
    let list = brep.edge_list();
    let all = list.iter().map(distance);
    all.enumerate()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap()
        .0
}

/// A tangent chain chamfered 1 and 2 is one chamfer: the tilted cylinder's elliptical rim, in
/// two arcs whose middles have different wedges (the kernel case
/// `2893-chamfer-two-distances-elliptical-rim`), measured at the narrowest wedge at which no
/// station folds: the wedge opens to 90 degrees plus the tilt at the rim's far tip, inside an
/// arc, and the smallest setback there keeps 1000 edge tolerances. Its one pair of offsets
/// puts the ball's centre on the cylinder of radius 5 less the wall's offset, at the
/// top's offset under the plane z = 10, so the top's rail is that cylinder's ellipse there,
/// lifted to the plane: semi-axes `R` along y and `R sqrt(5) / 2` along x, centred at
/// x = 10 less half the top's offset (the axis runs 1 along x to 2 along z).
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_tangent_chain_is_one_chamfer_measured_where_no_station_folds() {
    const SQRT_5: f64 = 2.236_067_977_499_79;
    all_fixes();
    let case = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/kernel_cases/fillets/",
        "2893-chamfer-two-distances-elliptical-rim.bodies.json.gz"
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(case).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b12"].clone()).unwrap();
    let (b, _) = Body::from_record(&record, true).unwrap();
    let up = [0.0, 0.0, 1.0];
    let rim = [
        nearest_edge(
            &b,
            [12.397279543126551, 4.505296518190091, 9.999999999999998],
        ),
        nearest_edge(
            &b,
            [7.602194600606031, -4.505901858302108, 9.999999999999998],
        ),
    ];
    assert_ne!(rim[0], rim[1]);
    // each arc's faces at its middle: one is the top, whose normal is up
    let mut top = 0;
    for e in rim {
        let [a, c] = edge_faces_along(&b, e, up).unwrap();
        let t = if a.1 > c.1 { a } else { c };
        assert!(rel(t.1, 1.0) < 1e-12);
        top = t.0;
    }
    let (_k, before) = kernel_copy(&b);
    let tolerance = rim
        .iter()
        .map(|&e| edge::tolerance(&before, before.edge_list()[e]).unwrap())
        .fold(0.0, f64::max);
    // At the widest wedge cos w = -1 / sqrt 5, sin w = 2 / sqrt 5. The kernel encloses it from
    // above, its normals' cosine within a gap of `tolerance / (1 + 2)`, so the cos w it uses lies
    // in [cos w - gap, cos w]; `unfolded_wedge`'s bound at each end of that range gives the rail's
    // ellipse at each end, and the built one lies between.
    let gap = tolerance / 3.0;
    let ellipse = |cos_w: f64| {
        let sin_w = (1.0 - cos_w * cos_w).sqrt();
        let c = (1.0 + 2.0 * cos_w - 1000.0 * tolerance * sin_w) / (2.0 + cos_w);
        let s = c.acos().sin();
        let (r_top, r_wall) = ((2.0 - c) / s, (1.0 - 2.0 * c) / s);
        let radius = 5.0 - r_wall;
        let half_x = radius * SQRT_5 / 2.0;
        let middle = 10.0 - r_top / 2.0;
        [radius, middle + half_x, middle - half_x]
    };
    let ends = [ellipse(-1.0 / SQRT_5), ellipse(-1.0 / SQRT_5 - gap)];
    chamfer_sides(&b, &rim, &[top, top], 1.0, ChamferSecond::Distance(2.0)).unwrap();
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report:?}");
    let (k, made) = kernel_copy(&b);
    let bx = made
        .face_list()
        .iter()
        .find_map(|&f| {
            let bx = face_bounding_box(&k, &made, f, true).ok()?;
            ((bx.min().z - 10.0).abs() < 1e-9 && (bx.max().z - 10.0).abs() < 1e-9).then_some(bx)
        })
        .unwrap();
    // each measure between its values at the two ends, to 1e-8 of it
    let between = |got: f64, i: usize| {
        let (a, b) = (ends[0][i], ends[1][i]);
        got >= a.min(b) - 1e-8 * a.abs() && got <= a.max(b) + 1e-8 * a.abs()
    };
    assert!(between(bx.max().y, 0), "{} {ends:?}", bx.max().y);
    assert!(between(-bx.min().y, 0), "{} {ends:?}", bx.min().y);
    assert!(between(bx.max().x, 1), "{} {ends:?}", bx.max().x);
    assert!(between(bx.min().x, 2), "{} {ends:?}", bx.min().x);
}

#[test]
fn a_round_larger_than_its_faces_is_refused_with_the_size_that_fits() {
    all_fixes();
    // a 2 thick wall, both its top edges rounded 1.5: the two rounds' bands overlap on the top
    let b = make_box([0.0; 3], [30.0, 2.0, 10.0]).unwrap();
    let edges = [
        edge_at(&b, [15.0, 0.0, 10.0]),
        edge_at(&b, [15.0, 2.0, 10.0]),
    ];
    let r = blend(&b, Blend::Round, &edges, &[1.5], &[], Corners::Filled);
    match r {
        Err(Error::TooLarge { size, kind, at, .. }) => {
            assert_eq!(kind, FitKind::Radius);
            assert!(size > 0.0 && size <= 1.0 + 1e-9, "{size}");
            let at = at.unwrap();
            assert!((at[2] - 10.0).abs() < 1e-9, "{at:?}");
        }
        Err(Error::NoRoom) => {}
        other => panic!("{other:?}"),
    }
    // refused: the body as it came in
    assert!(rel(volume(&b), 600.0) < 1e-12);
}

/// #18190: an r 1.85 rod notched by an r 0.5 disc centred on the rod's surface, at
/// (0, -1.85), from z = 5 to 15; the notch's floor and roof edges are arcs that end on the
/// rod's side. Rounded at the notch's own radius, the ball's centre stays on the notch's axis
/// and it touches the floor (roof) at the one point below it, which lies on the rod's side:
/// the round's ends in the side run down to that point, so it would take the whole floor.
/// Refused as a collapsed offset, naming a radius below 0.5 (open), at which it builds, not
/// failed while intersecting the round with the side.
#[test]
fn a_round_at_a_notch_s_radius_ending_on_a_side_through_its_axis_names_the_size_below() {
    all_fixes();
    let b = built(|k| {
        let rod = create_cylinder(k, Vec3::new(0.0, 0.0, 0.0), 1.85, 20.0)?;
        let disc = create_cylinder(k, Vec3::new(0.0, -1.85, 5.0), 0.5, 10.0)?;
        boolean_difference(k, rod, disc)
    });
    let before = volume(&b);
    let edges = [
        edge_at(&b, [0.0, -1.35, 5.0]),
        edge_at(&b, [0.0, -1.35, 15.0]),
    ];
    let size = match blend(&b, Blend::Round, &edges, &[0.5], &[], Corners::Filled) {
        Err(Error::TooLarge {
            size, kind, why, ..
        }) => {
            let got = format!("{size} {kind:?} {why:?}");
            assert_eq!(kind, FitKind::Radius, "{got}");
            assert_eq!(
                why,
                Some(solidcore_ops::RoomReason::OffsetCollapses),
                "{got}"
            );
            assert!((0.49..0.5).contains(&size), "{got}");
            size
        }
        other => panic!("{other:?}"),
    };
    assert!(rel(volume(&b), before) < 1e-12);
    blend(&b, Blend::Round, &edges, &[size], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    assert!(volume(&b) > before);
}

/// A body the kernel sweeps along +z, `height` high, from a closed profile in z = 0.
fn swept(profile: &[solidcore::curve::curve::Curve], height: f64) -> Body {
    let mut k = Kernel::default();
    let refs: Vec<Option<&solidcore::curve::curve::Curve>> = profile.iter().map(Some).collect();
    let b = solidcore::sweep::api_brep::create_linear_sweep(
        &mut k,
        &refs,
        Vec3::new(0.0, 0.0, 1.0),
        height,
        true,
    )
    .unwrap();
    let store = AttributeStore::default();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &loopuse::notify_ref,
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    Body::from_smb(
        write_brep_to_file(&k, Some(&b), &env, false).unwrap(),
        false,
    )
    .unwrap()
}

fn segment(a: (f64, f64), b: (f64, f64)) -> solidcore::curve::curve::Curve {
    solidcore::curve::curve::Curve::from(
        solidcore::primitive::api_curves::create_line_segment(
            Vec3::new(a.0, a.1, 0.0),
            Vec3::new(b.0, b.1, 0.0),
        )
        .unwrap(),
    )
}

/// The arc about `c` of radius `r` from `from` to `to` degrees.
fn arc_deg(c: (f64, f64), r: f64, from: f64, to: f64) -> solidcore::curve::curve::Curve {
    solidcore::curve::curve::Curve::from(
        solidcore::primitive::api_curves::create_arc(Vec3::new(c.0, c.1, 0.0), r, from, to)
            .unwrap(),
    )
}

/// Half a degree, in radians.
const HALF_DEGREE: f64 = 0.008_726_646_259_971_648;

/// A slot 2 high: two straights 10 long, 4 apart, joined by half circles of radius 2.
fn slot() -> Body {
    swept(
        &[
            segment((0.0, 0.0), (10.0, 0.0)),
            arc_deg((10.0, 2.0), 2.0, -90.0, 90.0),
            segment((10.0, 4.0), (0.0, 4.0)),
            arc_deg((0.0, 2.0), 2.0, 90.0, 270.0),
        ],
        2.0,
    )
}

/// The slot's top outline is one tangent chain: from its front straight, all four top edges;
/// a box edge runs on into nothing. Indices are checked before the kernel.
#[test]
fn the_tangent_chain_runs_round_the_slot() {
    all_fixes();
    let b = slot();
    let front = edge_at(&b, [5.0, 0.0, 2.0]);
    let chain = tangent_chain(&b, None, &[front], &[], HALF_DEGREE).unwrap();
    assert!(chain.undecided.is_empty(), "{chain:?}");
    let chain = chain.members;
    assert_eq!(chain.len(), 4, "{chain:?}");
    assert_eq!(chain[0], front);
    for p in [[5.0, 4.0, 2.0], [12.0, 2.0, 2.0], [-2.0, 2.0, 2.0]] {
        assert!(chain.contains(&edge_at(&b, p)), "{p:?}");
    }
    let bx = block();
    let e = top_front(&bx);
    let chain = |edges: &[usize], angle: f64| tangent_chain(&bx, None, edges, &[], angle);
    assert_eq!(chain(&[e], HALF_DEGREE).unwrap().members, vec![e]);
    assert_eq!(chain(&[99], 0.01), Err(Error::Argument));
    assert_eq!(chain(&[e], 0.0), Err(Error::Argument));
    assert_eq!(chain(&[], 0.01), Err(Error::Argument));
}

/// The slot's seam, where its straight side runs into its round end, has no blend: its faces
/// meet tangentially all along it. Refused with its index, the body as it came in.
#[test]
fn an_edge_with_no_blend_is_named() {
    all_fixes();
    let b = slot();
    let seam = edge_at(&b, [10.0, 0.0, 1.0]);
    let before = volume(&b);
    let r = blend(&b, Blend::Round, &[seam], &[0.5], &[], Corners::Filled);
    assert_eq!(
        r,
        Err(Error::EdgeNotBlendable {
            edge: seam,
            reason: Unblendable::Smooth,
        })
    );
    assert!(rel(volume(&b), before) < 1e-12);
}

/// A crease whose faces turn by atan(0.0875) (5 degrees) at x = 10: a round of 1e-4 lies within
/// the edge's tolerance of it and is refused, naming the smallest radius that rounds it; a
/// round of 0.5 builds.
#[test]
fn a_round_too_small_to_change_the_part_is_refused() {
    all_fixes();
    let crease = || {
        swept(
            &[
                segment((0.0, 0.0), (10.0, 0.0)),
                segment((10.0, 0.0), (20.0, 0.875)),
                segment((20.0, 0.875), (20.0, 10.0)),
                segment((20.0, 10.0), (0.0, 10.0)),
                segment((0.0, 10.0), (0.0, 0.0)),
            ],
            5.0,
        )
    };
    let b = crease();
    let e = edge_at(&b, [10.0, 0.0, 2.5]);
    let before = volume(&b);
    match blend(&b, Blend::Round, &[e], &[1e-4], &[], Corners::Filled) {
        Err(Error::RoundTooSmall {
            edge,
            turn,
            tolerance,
            smallest,
        }) => {
            assert_eq!(edge, e);
            // atan(0.0875), to the kernel's evaluation
            assert!((turn - 0.087_277_712_949_461_45).abs() < 1e-12, "{turn}");
            assert!(tolerance > 0.0 && smallest > 1e-4, "{tolerance} {smallest}");
        }
        other => panic!("{other:?}"),
    }
    assert!(rel(volume(&b), before) < 1e-12);
    let b = crease();
    blend(&b, Blend::Round, &[e], &[0.5], &[], Corners::Filled).unwrap();
    assert!(volume(&b) < before);
}

/// #3351: on a square edge of a body of tolerance 1e-5, a round narrower across its faces
/// than eight tolerances (its chord, r*sqrt(2), below 8e-5) is refused as too small, naming
/// the smallest that builds, 8e-5/sqrt(2) and one part in a million: 2e-5 lies within
/// tolerance of the edge, 2.5e-5 failed with no fillet faces made and 4e-5 failed matching
/// its edges, untyped. A round of the smallest named builds exact, and so do rounds just
/// above it. So on a 120 degree turn, where a chord of exactly eight tolerances fails.
#[test]
fn a_round_narrower_than_eight_tolerances_is_refused_naming_one_that_builds() {
    all_fixes();
    let floor = 8e-5 / std::f64::consts::SQRT_2 * (1.0 + 1e-6);
    let mut smallest = 0.0;
    for r in [2e-5, 2.5e-5, 4e-5, 5.5e-5] {
        let b = block();
        let e = edge_at(&b, [15.0, 0.0, 0.0]);
        match blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled) {
            Err(Error::RoundTooSmall {
                edge, smallest: s, ..
            }) => {
                assert_eq!(edge, e);
                assert!((s - floor).abs() < 1e-9 * floor, "{r} {s}");
                smallest = s;
            }
            other => panic!("{r}: {other:?}"),
        }
        assert!(rel(volume(&b), ABC) < 1e-12, "{r}");
    }
    for r in [smallest, 5.7e-5, 6e-5] {
        let b = block();
        let e = edge_at(&b, [15.0, 0.0, 0.0]);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
            .unwrap_or_else(|err| panic!("a round of {r} builds: {err}"));
        let removed = 30.0 * r * r * (1.0 - std::f64::consts::FRAC_PI_4);
        assert!(
            ((ABC - volume(&b)) - removed).abs() < 1e-3 * removed,
            "{r} {}",
            volume(&b)
        );
        assert_eq!(b.counts().unwrap(), [7, 15, 10], "{r}");
    }
    // a prism whose edge at the origin turns its faces by 120 degrees
    let a = 10.0 / 60f64.to_radians().tan();
    let prism = || {
        swept(
            &[
                segment((0.0, 0.0), (20.0, 0.0)),
                segment((20.0, 0.0), (20.0, 10.0)),
                segment((20.0, 10.0), (a, 10.0)),
                segment((a, 10.0), (0.0, 0.0)),
            ],
            10.0,
        )
    };
    let turn = 120f64.to_radians();
    let floor = 8e-5 / (2.0 * (0.5 * turn).sin()) * (1.0 + 1e-6);
    let b = prism();
    let before = volume(&b);
    let e = edge_at(&b, [0.0, 0.0, 5.0]);
    match blend(&b, Blend::Round, &[e], &[4.5e-5], &[], Corners::Filled) {
        Err(Error::RoundTooSmall {
            edge, smallest: s, ..
        }) => {
            assert_eq!(edge, e);
            assert!((s - floor).abs() < 1e-9 * floor, "{s}");
            smallest = s;
        }
        other => panic!("4.5e-5 at 120 degrees: {other:?}"),
    }
    assert!(rel(volume(&b), before) < 1e-12);
    for r in [smallest, 4.7e-5] {
        let b = prism();
        let e = edge_at(&b, [0.0, 0.0, 5.0]);
        blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled)
            .unwrap_or_else(|err| panic!("a round of {r} at 120 degrees builds: {err}"));
        let removed = 10.0 * r * r * ((0.5 * turn).tan() - 0.5 * turn);
        assert!(
            ((before - volume(&b)) - removed).abs() < 1e-3 * removed,
            "{r}"
        );
    }
}

/// #17511: a round of 8 on a quarter circle of radius 10 that meets a straight side 16.7
/// degrees off tangent. On the top, the round of the arc touches the circle of radius 2 about
/// the arc's centre and the round of the side touches the line 8 in from it; they cross twice,
/// 2.4 apart, and the corner is where the arc's own round meets the side's: on the arc's side of
/// its end. The arc is shorter than twice the radius, so a guess measured along the edges lies
/// near the arc's far end, and the solve found the other crossing, a ball past the arc's end,
/// whose mitre with the side's round did not reach it. The crossing nearest the vertex builds.
#[test]
fn a_round_on_an_arc_shorter_than_its_diameter_meets_the_next_side_at_its_own_crossing() {
    all_fixes();
    let b = swept(
        &[
            segment((0.0, 0.0), (30.0, 0.0)),
            segment((30.0, 0.0), (30.0, 20.0)),
            arc_deg((20.0, 20.0), 10.0, 0.0, 90.0),
            segment((20.0, 30.0), (0.0, 24.0)),
            segment((0.0, 24.0), (0.0, 0.0)),
        ],
        10.0,
    );
    let half = 0.5_f64.sqrt() * 10.0;
    let edges = [
        edge_at(&b, [15.0, 0.0, 10.0]),
        edge_at(&b, [30.0, 10.0, 10.0]),
        edge_at(&b, [20.0 + half, 20.0 + half, 10.0]),
        edge_at(&b, [10.0, 27.0, 10.0]),
        edge_at(&b, [0.0, 12.0, 10.0]),
    ];
    let before = volume(&b);
    blend(&b, Blend::Round, &edges, &[8.0], &[], Corners::Filled).unwrap();
    assert!(volume(&b) < before);
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    // the side's unit direction from the arc's end and its normal into the part
    let len = 436.0_f64.sqrt();
    let (t, n) = ((-20.0 / len, -6.0 / len), (6.0 / len, -20.0 / len));
    // the round's contact circle about (20, 20) meets the side's contact line 8 in from it
    let h = 8.0 - 200.0 / len;
    let w = (4.0 - h * h).sqrt();
    let crossing = |sign: f64| {
        [
            20.0 + h * n.0 + sign * w * t.0,
            20.0 + h * n.1 + sign * w * t.1,
            10.0,
        ]
    };
    let nearest = |p: [f64; 3]| {
        let (_, brep) = kernel_copy(&b);
        brep.vertex_list()
            .iter()
            .map(|&v| {
                let q = solidcore::query::api_queries::vertex_get_point(&brep, v).unwrap();
                ((q.x - p[0]).powi(2) + (q.y - p[1]).powi(2) + (q.z - p[2]).powi(2)).sqrt()
            })
            .fold(f64::INFINITY, f64::min)
    };
    // on the arc's side of its end (x > 20): a vertex; past it: none
    assert!(
        nearest(crossing(-1.0)) < 1e-6,
        "{}",
        nearest(crossing(-1.0))
    );
    assert!(nearest(crossing(1.0)) > 1.0, "{}", nearest(crossing(1.0)));
}

/// solidcore#2250's box: 40 x 30 x 20, its top back edge chamfered 5 (V0 23500); A, the
/// chamfer's top edge (a 135 degree wedge), and B, the bottom front edge (90 degrees), rounded
/// curvature-continuously r 2 in one call, in either order. Each round takes off its own ball's
/// area, r^2 (cot(a/2) - (pi - a)/2) per unit length (0.0860579227 for A, 0.8584073464 for
/// B), along 40: the kernel resolves the equal-area thumbweight from each station's own wedge.
#[test]
fn curvature_rounds_over_mixed_wedges_each_take_their_own_balls_area() {
    all_fixes();
    let per_length = 0.086_057_922_7 + 0.858_407_346_4;
    let run = |a_first: bool| -> f64 {
        let b = make_box([0.0; 3], [40.0, 30.0, 20.0]).unwrap();
        let back = edge_at(&b, [20.0, 30.0, 20.0]);
        blend(&b, Blend::Chamfer, &[back], &[5.0], &[], Corners::Filled).unwrap();
        assert!(rel(volume(&b), 23500.0) < 1e-9);
        let a = edge_at(&b, [20.0, 25.0, 20.0]);
        let bottom = edge_at(&b, [20.0, 0.0, 0.0]);
        let edges = if a_first { [a, bottom] } else { [bottom, a] };
        blend(&b, Blend::Curvature, &edges, &[2.0], &[], Corners::Filled).unwrap();
        23500.0 - volume(&b)
    };
    let ab = run(true);
    let ba = run(false);
    assert!((ab - 40.0 * per_length).abs() < 1e-5, "[A, B] took {ab}");
    assert!((ba - 40.0 * per_length).abs() < 1e-5, "[B, A] took {ba}");
    assert!((ab - ba).abs() < 1e-7, "{ab} {ba}");
}

/// #3145: a chamfer too large for its faces is refused with the distance that builds, which the
/// kernel reads through the chamfer's definition (its offsets linear in its sizes at the edge's
/// wedge), and that distance builds. A 2 thick wall, both its top edges chamfered 1.5 at
/// equal distances: square edges, so each chamfer's setback is its distance, and the two bands
/// on the top meet when the setbacks are 1 each: the fit is 1. Chamfered at it, each edge
/// loses the prism of the right triangle of side 1 along the wall's 30: `2 * 30 / 2` removed.
#[test]
fn a_chamfer_too_large_is_refused_with_the_distance_that_builds() {
    all_fixes();
    let b = make_box([0.0; 3], [30.0, 2.0, 10.0]).unwrap();
    let edges = [
        edge_at(&b, [15.0, 0.0, 10.0]),
        edge_at(&b, [15.0, 2.0, 10.0]),
    ];
    let size = match blend(&b, Blend::Chamfer, &edges, &[1.5], &[], Corners::Filled) {
        Err(Error::TooLarge { size, kind, .. }) => {
            assert_eq!(kind, FitKind::Distance);
            size
        }
        other => panic!("{other:?}"),
    };
    // the kernel's fit read through the chamfer's offsets and back: lines, so no quadrature,
    // only the fit's and the conversions' rounding (measured 1.1e-13)
    assert!((size - 1.0).abs() <= 1e-12, "{size}");
    let before = volume(&b);
    blend(&b, Blend::Chamfer, &edges, &[size], &[], Corners::Filled).unwrap();
    let removed = before - volume(&b);
    let want = 30.0 * size * size;
    assert!(rel(removed, want) <= 1e-9, "{removed} vs {want}");
}

/// The two selected rims of a quadratic-ended extrusion approach across a 0.6 mm
/// terminal strip (#19108). A fixed-angle refusal names the first distance and a
/// point on the limiting top face; explicit two-distance input keeps its common scale.
#[test]
fn a_distance_angle_chamfer_on_a_curved_tip_names_its_distance_edge_and_limiting_face() {
    use solidcore::nurbs::b_spline_curve::BSplineCurve;
    use solidcore::nurbs::math::CPoint;
    use solidcore_ops::RoomReason;

    all_fixes();
    let outline = || {
        let quadratic = BSplineCurve::from_data(
            3,
            2,
            5,
            2,
            &[
                CPoint::new(10.0, 0.6, 0.0, 1.0),
                CPoint::new(5.0, 0.6, 0.0, 1.0),
                CPoint::new(0.0, 5.0, 0.0, 1.0),
            ],
            &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
        .unwrap();
        let native = swept(
            &[
                segment((0.0, 0.0), (10.0, 0.0)),
                segment((10.0, 0.0), (10.0, 0.6)),
                solidcore::curve::curve::Curve::from(quadratic),
                segment((0.0, 5.0), (0.0, 0.0)),
            ],
            10.0,
        );
        // Application bodies pass through the record boundary: a native curved
        // extrusion wall returns as a B-spline with the same translated net.
        let (record, _) = native.to_record().unwrap();
        Body::from_record(&record, true).unwrap().0
    };
    let angle = 80.0_f64.to_radians();
    let mut distance_fit = None;
    for second in [
        ChamferSecond::Angle(angle),
        ChamferSecond::Distance(angle.tan()),
    ] {
        let b = outline();
        let before = b.to_record().unwrap().0;
        let picks = [edge_at(&b, [5.0, 0.0, 10.0]), edge_at(&b, [5.0, 1.7, 10.0])];
        let e = chamfer_sides_facing(&b, &picks, 1.0, second, [0.0, 0.0, 1.0]).unwrap_err();
        let Error::TooLarge {
            size,
            kind,
            at: Some(at),
            face: Some(face),
            why: Some(RoomReason::BandsOverlap),
        } = e
        else {
            panic!("{e:?}");
        };
        let expected_kind = match second {
            ChamferSecond::Angle(_) => FitKind::Distance,
            ChamferSecond::Distance(_) => FitKind::Scale,
        };
        assert_eq!(kind, expected_kind);
        // The terminal event is 0.6 / 2; the named open fit is below it.
        assert!(size > 0.29 && size < 0.3, "{size}");
        assert!((at[2] - 10.0).abs() < 1e-12, "edge {at:?}");
        assert!((face[2] - 10.0).abs() < 1e-12, "limiting top face {face:?}");
        assert!(face[0] > 0.0 && face[0] < 10.0 && face[1] > 0.0, "{face:?}");
        assert_eq!(
            b.to_record().unwrap().0,
            before,
            "a refusal leaves its input intact"
        );
        if kind == FitKind::Distance {
            distance_fit = Some(size);
        } else {
            assert!((size - distance_fit.unwrap()).abs() < 1e-12);
        }
    }
}

/// #3145: a setback longer than an edge at a real set-back corner is the kernel's refusal before
/// the trace, naming the largest that fits: the block's corner at (30, 0, 10) with its three
/// edges rounded 1 and set back 50, its shortest edge 10 long. A setback of 10 would leave that
/// edge's round no length (an open bound), so the size named is 10 less a thousand of the
/// edge's tolerances; set back that, the block builds, and just above it is refused. Without
/// the check the trace refused first, as a round too large (radius 19.99, the corners taking a
/// whole edge).
#[test]
fn a_setback_too_long_at_a_corner_is_refused_by_the_kernel() {
    all_fixes();
    let b = block();
    let edges = [
        edge_at(&b, [15.0, 0.0, 10.0]),
        edge_at(&b, [30.0, 10.0, 10.0]),
        edge_at(&b, [30.0, 0.0, 5.0]),
    ];
    let corner = vertex_at(&b, [30.0, 0.0, 10.0]);
    let set_back = |distance: f64| {
        [SetBack {
            vertex: corner,
            distance,
        }]
    };
    let tol = {
        let (_k, kb) = kernel_copy(&b);
        edge::tolerance(&kb, kb.edge_list()[edges[2]]).unwrap()
    };
    let named = match blend(
        &b,
        Blend::Round,
        &edges,
        &[1.0],
        &set_back(50.0),
        Corners::Filled,
    ) {
        Err(Error::SetbackTooLong { vertex, largest }) => {
            assert_eq!(vertex, corner);
            // the least of the three edges' lengths (lines: exact), the far ends set back none,
            // less the fit margin
            let want = 10.0 - 1000.0 * tol;
            assert!((largest - want).abs() <= 1e-12, "{largest} against {want}");
            largest
        }
        other => panic!("{other:?}"),
    };
    // just above the named size: refused by the same test, the block unchanged
    match blend(
        &b,
        Blend::Round,
        &edges,
        &[1.0],
        &set_back(named + 1e-9),
        Corners::Filled,
    ) {
        Err(Error::SetbackTooLong { vertex, .. }) => assert_eq!(vertex, corner),
        other => panic!("{other:?}"),
    }
    assert!(rel(volume(&b), 6000.0) < 1e-12);
    // at it: the block builds (the rounds take volume)
    blend(
        &b,
        Blend::Round,
        &edges,
        &[1.0],
        &set_back(named),
        Corners::Filled,
    )
    .unwrap();
    assert!(volume(&b) < 6000.0);
}

/// Cancel stops a long fillet promptly (solidcore#3247): a 2 mm round along one helical edge of
/// a 20-turn square coil, with its tangent chain (the whole edge), runs for minutes. Measured in
/// the token's polls, not in time: a token cancelled from its `m`-th poll
/// (`CancelToken::after_polls`) stops the fillet there. It returns `Cancelled`, which also shows
/// the fillet was still running at poll `m`, and the token is polled exactly once after the poll
/// that cancelled it: by the session as the call returns, asking whether it was cancelled
/// (`session::run_with`, or `with_body` deciding whether to keep its edit). Any further poll
/// would be work done after the cancel. `m` is a quarter of the 15,473 polls the fillet made in
/// its first second, measured on SolidCraft 9a342278 (cancel-timing round 1). Should the fillet
/// get fast enough to finish before `m`, this fails with `Ok`: then pick a longer case.
#[test]
fn a_cancelled_long_fillet_stops_promptly() {
    use solidcore_ops::cancel::{within, CancelToken};
    use solidcore_ops::sweep::{screw_sweep, Curve};
    all_fixes();
    let corners = [
        [19.0, 0.0, 0.0],
        [32.0, 0.0, 0.0],
        [32.0, 0.0, 13.0],
        [19.0, 0.0, 13.0],
    ];
    let square: Vec<Curve> = (0..4)
        .map(|i| Curve::Line {
            start: corners[i],
            end: corners[(i + 1) % 4],
        })
        .collect();
    let coil = screw_sweep(
        &square,
        [0.0; 3],
        [0.0, 0.0, 1.0],
        15.24,
        20.0,
        None,
        15.24e-4,
    )
    .unwrap();
    // the inner lower helix half a turn in, whichever way the sweep turns
    let (k, brep) = kernel_copy(&coil);
    let q = Vec3::new(-19.0, 0.0, 15.24 / 2.0);
    let edge = (0..brep.edge_list().len())
        .min_by(|&a, &b| {
            let d = |i: usize| {
                edge_closest_point(&k, &brep, brep.edge_list()[i], q)
                    .unwrap()
                    .closest
                    .distance
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    let chain = tangent_chain(&coil, None, &[edge], &[], 0.1)
        .unwrap()
        .members;
    let m: u64 = 15_473 / 4;
    let token = CancelToken::after_polls(m);
    let r = within(&token, || {
        blend(&coil, Blend::Round, &chain, &[2.0], &[], Corners::Filled)
    });
    eprintln!("the fillet cancelled at {m}, polled {:?}", token.polls());
    assert_eq!(r.map(|_| ()), Err(Error::Cancelled));
    assert_eq!(token.polls(), Some(m + 1));
}

/// A failure the kernel types as not supported, after the session noted no room with a fit
/// (or an unsupported vertex), is its own cause: [`solidcore_ops::blend::session_refusal`]
/// gives it back rather than sizing it from the earlier note (SolidCraft's own mapping made it
/// `TooLarge` before it took ops'). The note is real: a 20 cube's three corner edges rounded
/// 20, refused naming 19.99 (the case of SolidCraft's `a_cliff_overflow_the_fork_refuses_has_no_room`).
#[test]
fn a_kernel_unsupported_after_a_no_room_note_is_its_own_cause() {
    use solidcore::blend::fillet_executive::session::FilletSession;
    use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
    use solidcore::kernel::UpstreamFixes;
    use solidcore::SolidError;
    use solidcore_ops::blend::{session_refusal, Refusal, SessionCall};
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let mut b = create_box(&mut k, Vec3::ZERO, 20.0, 20.0, 20.0).unwrap();
    let at = [
        Vec3::new(10.0, 0.0, 0.0),
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::new(0.0, 0.0, 10.0),
    ];
    let edges: Vec<EdgeId> = at
        .iter()
        .map(|&q| {
            b.edges()
                .ids()
                .min_by(|&x, &y| {
                    let d = |e| edge_closest_point(&k, &b, e, q).unwrap().closest.distance;
                    d(x).total_cmp(&d(y))
                })
                .unwrap()
        })
        .collect();
    let mut s = FilletSession::new(&mut k, &mut b).unwrap();
    let circular = [FilletSurfaceGeneratorType::Circular];
    s.prepare_per_edge_constant_radius(&mut k, &edges, &[20.0; 3], &circular, 1, 1.0)
        .unwrap();
    s.create_corners(&k, None).unwrap();
    let refused = s.do_filleting(&mut k, None).unwrap_err();
    // the note: no room, with the fit
    let sized = session_refusal(&s, refused, SessionCall::default()).unwrap();
    let Refusal::TooLarge { size, .. } = sized.on(&k, s.target()).unwrap() else {
        panic!("not sized")
    };
    assert!((size - 19.99).abs() < 1e-9, "{size}");
    let unsupported = SolidError::Unsupported {
        operation: "test",
        reason: "a construction the kernel does not build",
    };
    for corner in [false, true] {
        let call = SessionCall {
            corner,
            ..SessionCall::default()
        };
        assert_eq!(
            session_refusal(&s, unsupported.clone(), call).map(|_| ()),
            Err(unsupported.clone()),
            "corner {corner}"
        );
    }
}

/// An L of legs 40 and width 20, 10 high: its inner upright edge at (20, 20) is concave between
/// two flat faces and ends on the flat top and bottom, so a round can be carried on past
/// either face: each is named by its index, with the slab's frame at the edge's start, the
/// edge's 10 along it, and `along` × `across` the face's outward normal (+x or +y). A convex
/// edge has none; an index past the edges and a radius that is not positive are refused before
/// the kernel.
#[test]
fn the_faces_a_round_is_carried_on_past_are_the_kernels() {
    all_fixes();
    let l = [
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 20.0),
        (20.0, 20.0),
        (20.0, 40.0),
        (0.0, 40.0),
    ];
    let profile: Vec<_> = (0..l.len())
        .map(|i| segment(l[i], l[(i + 1) % l.len()]))
        .collect();
    let b = swept(&profile, 10.0);
    let inner = nearest_edge(&b, [20.0, 20.0, 5.0]);
    let found = solidcore_ops::carried_faces(&b, inner, 6.0).unwrap();
    assert_eq!(found.len(), 2);
    assert_ne!(found[0].face, found[1].face);
    let mut normals = Vec::new();
    for c in &found {
        assert!(c.face < b.to_record().unwrap().0.faces.len());
        assert!((c.origin[0] - 20.0).abs() < 1e-9 && (c.origin[1] - 20.0).abs() < 1e-9);
        assert_eq!(
            c.outline,
            [
                [0.0, -c.reach],
                [10.0, -c.reach],
                [10.0, c.reach],
                [0.0, c.reach]
            ]
        );
        assert_eq!(c.depth, [-c.reach, 0.0]);
        let (a, x) = (
            Vec3::new(c.along[0], c.along[1], c.along[2]),
            Vec3::new(c.across[0], c.across[1], c.across[2]),
        );
        normals.push(a.cross(x));
    }
    let is = |n: Vec3, x: f64, y: f64| (n - Vec3::new(x, y, 0.0)).length() < 1e-9;
    assert!(normals.iter().any(|&n| is(n, 1.0, 0.0)) && normals.iter().any(|&n| is(n, 0.0, 1.0)));
    let convex = nearest_edge(&b, [40.0, 0.0, 5.0]);
    assert!(solidcore_ops::carried_faces(&b, convex, 6.0)
        .unwrap()
        .is_empty());
    assert!(matches!(
        solidcore_ops::carried_faces(&b, 999, 6.0),
        Err(Error::Argument)
    ));
    assert!(matches!(
        solidcore_ops::carried_faces(&b, inner, 0.0),
        Err(Error::Argument)
    ));
}

/// The L of legs 40 and width 20, `height` high, less what lies above the plane through
/// (`x0`, `z0`) and (`x1`, `z1`) in the side view (x across, z up), from `x0` to `x1`.
fn l_cut_by_a_slope(height: f64, [x0, z0]: [f64; 2], [x1, z1]: [f64; 2]) -> Body {
    let l = [
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 20.0),
        (20.0, 20.0),
        (20.0, 40.0),
        (0.0, 40.0),
    ];
    let profile: Vec<_> = (0..l.len())
        .map(|i| segment(l[i], l[(i + 1) % l.len()]))
        .collect();
    let line = |a: [f64; 2], b: [f64; 2]| Curve::Line {
        start: [a[0], -1.0, a[1]],
        end: [b[0], -1.0, b[1]],
    };
    let top = z0.max(z1) + height;
    let above = extrude(
        &[
            line([x0, z0], [x1, z1]),
            line([x1, z1], [x1, top]),
            line([x1, top], [x0, top]),
            line([x0, top], [x0, z0]),
        ],
        [0.0, 1.0, 0.0],
        42.0,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap();
    solidcore_ops::boolean::boolean(
        swept(&profile, height),
        above,
        solidcore_ops::BooleanOp::Subtract,
    )
    .unwrap()
    .unwrap()
}

/// The L above, 30 high, its top cut off by the plane z = 15 + x / 4: the inner upright edge
/// at (20, 20) now runs from the flat bottom to that plane, 20 up. The plane holds the normal
/// of the edge's face y = 20 and not that of its face x = 20, so a round is carried on past
/// the first only. Its slab's outline lies in y = 20 with two corners on the bottom and two
/// on the sloped plane. The traces of the bottom and the plane cross at x = -60, 80 from the
/// edge, on the side x falls: a round of 6 has its slab reach (6 + 80) / 2 = 43 that way, to
/// x = -23 where the plane is 9.25 up, and the full reach the other way. A round of 81 has
/// its contact line past the crossing and nothing to carry on.
///
/// Cut off instead by a plane that rises 2 for 1 (z = 2 x - 10, from x = 10 on), 63.4
/// degrees from square to the edge, past the 60 the kernel takes, the edge carries nothing.
#[test]
fn a_round_is_carried_on_between_a_flat_end_and_a_sloped_one() {
    all_fixes();
    let b = l_cut_by_a_slope(30.0, [-1.0, 14.75], [41.0, 25.25]);
    let inner = nearest_edge(&b, [20.0, 20.0, 10.0]);
    let found = solidcore_ops::carried_faces(&b, inner, 6.0).unwrap();
    assert_eq!(found.len(), 1);
    let c = found[0];
    let v = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
    let (along, across) = (v(c.along), v(c.across));
    assert!((along.cross(across) - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-9);
    let corners = c.outline.map(|[a, x]| v(c.origin) + along * a + across * x);
    let has = |x: f64, z: f64| {
        corners
            .iter()
            .any(|p| (*p - Vec3::new(x, 20.0, z)).length() < 1e-6)
    };
    let far = 20.0 + c.reach;
    assert!(
        has(-23.0, 0.0) && has(-23.0, 9.25) && has(far, 0.0) && has(far, 15.0 + far / 4.0),
        "{corners:?}"
    );
    assert_eq!(c.depth, [-c.reach, 0.0]);
    // past the edge's line, 5 out on the other face's side, between the same two ends
    let past = c
        .past_edge(5.0)
        .unwrap()
        .map(|[a, x]| v(c.origin) + along * a + across * x);
    let x = 20.0 + 5.0 * c.other_side * across.dot(Vec3::new(1.0, 0.0, 0.0));
    for (px, pz) in [(20.0, 0.0), (20.0, 20.0), (x, 0.0), (x, 15.0 + x / 4.0)] {
        assert!(
            past.iter()
                .any(|p| (*p - Vec3::new(px, 20.0, pz)).length() < 1e-6),
            "({px}, {pz}) in {past:?}"
        );
    }
    assert!(matches!(c.past_edge(f64::NAN), Err(Error::Argument)));
    assert_eq!(
        solidcore_ops::carried_faces(&b, inner, 79.0).unwrap().len(),
        1
    );
    assert!(solidcore_ops::carried_faces(&b, inner, 81.0)
        .unwrap()
        .is_empty());
    let steep = l_cut_by_a_slope(70.0, [10.0, 10.0], [41.0, 72.0]);
    let inner = nearest_edge(&steep, [20.0, 20.0, 10.0]);
    assert!(solidcore_ops::carried_faces(&steep, inner, 6.0)
        .unwrap()
        .is_empty());
}

/// The kernel rig's fix set (SolidCraft's preset, `solidcraft_fixes.rs`), as the rig runs its
/// entries.
fn rig_fixes() {
    solidcore_ops::session::set_thread_fixes(solidcraft_fixes::solidcraft_fix_names()).unwrap();
}

/// The ledge of the kernel case `10876-fillet-ledge-convex-and-concave-edges-at-bore-r0-5`:
/// its operand body `b255`.
fn ledge() -> Body {
    let case = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/kernel_cases/fillets/",
        "10876-fillet-ledge-convex-and-concave-edges-at-bore-r0-5.bodies.json.gz"
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(case).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b255"].clone()).unwrap();
    Body::from_record(&record, true).unwrap().0
}

/// The ledge's concave edge rounded first (r 0.5, alone: it ends flush on the bore's flat
/// wall x = 12), then its convex edge's remainder with the concave round's end rib on that
/// wall rounded as one chain: the second pass's trim leaks past its rails (the convex ball
/// would have to end where the chain runs into the sharp edge it leaves unselected). The
/// refusal keeps the kernel's class and statement, not a bare status 1001.
#[test]
fn a_ledge_rounded_concave_first_refuses_its_convex_chain_with_its_reason() {
    rig_fixes();
    let b = ledge();
    let concave = nearest_edge(&b, [15.0, 30.0, 33.0]);
    blend(&b, Blend::Round, &[concave], &[0.5], &[], Corners::Filled).unwrap();
    let h = 0.5 * std::f64::consts::FRAC_1_SQRT_2;
    let chain = [
        nearest_edge(&b, [12.0, 29.5 + h, 33.5 - h]),
        nearest_edge(&b, [12.0, 25.0, 33.0]),
    ];
    assert_ne!(chain[0], chain[1]);
    let got = blend(&b, Blend::Round, &chain, &[0.5], &[], Corners::Filled);
    match got {
        Err(Error::BlendFailed { status, phrase, .. }) => {
            assert_eq!(status, 1001);
            assert!(
                phrase.contains("the faces the rails enclose reach past the rails"),
                "{phrase}"
            );
        }
        other => panic!("not the trim's typed refusal: {other:?}"),
    }
}

/// `int_a^b sqrt(r^2 - x^2) dx`, in closed form.
fn disc_strip(a: f64, b: f64, r: f64) -> f64 {
    let f = |x: f64| 0.5 * x * (r * r - x * x).sqrt() + 0.5 * r * r * (x / r).asin();
    f(b) - f(a)
}

/// The ledge's operand volume, the kernel case's closed form (its `method`):
///
/// ```text
/// 15 (1728 + 162 pi) - 6 (15 * 48 + I(-7.5, 7.5; 18)) - 432 pi - 216 pi + 36 * 27 * 18
///   - 18 * 6 * 18 + 36 * 18 * 12 + 2916 pi - 18 (288 + 144 pi) - 3 * 72 pi - 6 * 2 I(9, 12; 12)
/// ```
///
/// with `I(a, b; R) = int_a^b sqrt(R^2 - x^2) dx`: 43913.957931372.
fn ledge_operand_volume() -> f64 {
    15.0 * (1728.0 + 162.0 * PI)
        - 6.0 * (15.0 * 48.0 + disc_strip(-7.5, 7.5, 18.0))
        - 432.0 * PI
        - 216.0 * PI
        + 36.0 * 27.0 * 18.0
        - 18.0 * 6.0 * 18.0
        + 36.0 * 18.0 * 12.0
        + 2916.0 * PI
        - 18.0 * (288.0 + 144.0 * PI)
        - 3.0 * 72.0 * PI
        - 6.0 * 2.0 * disc_strip(9.0, 12.0, 12.0)
}

/// The ledge corner (#10876) rounded r 0.5 at both edges as the blend corner's Roll-over, its
/// volume's change from the operand's. In the local frame u = x - 12, v = y - 30, w = z - 33
/// (the ledge top w = 0 for v < 0, the arch's front v = 0, the bore's flat wall u = 0 above
/// w = 0 and its round bottom, the circle of radius `RB` = 12 about u = -12, w = 0, below):
/// - the convex round (E1, between the ledge top and the bore's bottom, from y = 21 to 30)
///   takes its section `A1` over its length 9. Its axis A1 is at u = `ua` = sqrt(156) - 12,
///   w = -r (the ball touching the top and outside the bore: `(RB + ua)^2 + r^2 = (RB + r)^2`);
///   its section is the curvilinear triangle bounded by the top from (ua, 0) to the corner,
///   the bore's circle from the corner to its tangent point T with the round, and the round's
///   arc back: by Green's theorem `A1 = 1/2 (RB^2 (phi_t - sin phi_t) + loop_round)`, below;
/// - the concave round (E2, between the top and the arch's front, from x = 12 to 18) adds its
///   section `(1 - pi/4) r^2` over its remainder from u = ua (the convex round's rail on the
///   top) to 6;
/// - the torus about A1 (major radius 2 r, minor r) adds the material past that strip, as on
///   the planar twin ([`roll_over_material`]): `I = int_r^(2r) rho (r - sqrt(r^2 - (2r -
///   rho)^2)) theta_max(rho) d rho`, the meridian section of height `r - sqrt(..)` at distance
///   `rho` from A1 swept over the directions `theta` (from +w towards -u) the cliff keeps. The
///   cliff is the flat wall where its direction meets it above w = 0 (`rho sin theta = ua`,
///   for `rho` past W's distance `rho_w = sqrt(ua^2 + r^2)` from A1) and the bore's circle
///   below (`(RB + ua) sin theta + r cos theta = ((RB + r)^2 - RB^2 + rho^2) / (2 rho)`, for
///   `rho` up to `rho_w`); with `rho = 2r - r cos phi` the integrand is analytic on each side
///   of `phi_w`, and each side is integrated by two Gauss-Legendre rules (20 points on 8
///   panels, 30 on 16) that agree to 1e-14 relative (asserted): the reference is their mean.
fn ledge_rolled_change(r: f64) -> f64 {
    let rb = 12.0;
    let ua = ((rb + r) * (rb + r) - r * r).sqrt() - rb;
    // A1: the loop top (ua, 0) -> corner (0, 0) -> T along the bore's circle -> back along
    // the round's arc, counter-clockwise; 1/2 the integral of u dw - w du
    let d = rb + r;
    let phi_t = (-r / d).asin(); // T's angle about the bore's axis, below w = 0
    let along_bore = rb * rb * (phi_t - phi_t.sin());
    // the round's arc about (ua, -r) from T's direction psi_t to the top's tangent point at
    // pi/2: with u = ua + r cos psi, w = -r + r sin psi, u dw - w du = (ua r cos psi + r^2 -
    // r^2 sin psi) d psi, whose integral is ua r sin psi + r^2 psi + r^2 cos psi. T lies on the
    // line from the bore's axis to the round's centre, so its direction from the centre is
    // the opposite of (RB + ua, -r)
    let psi_t = r.atan2(-(ua + rb));
    let arc = |psi: f64| ua * r * psi.sin() + r * r * psi + r * r * psi.cos();
    let along_round = arc(PI / 2.0) - arc(psi_t);
    let a1 = 0.5 * (along_bore + along_round);
    let strip = (1.0 - PI / 4.0) * r * r;
    // the torus's material past the concave strip
    let big_r = 2.0 * r;
    let rho_w = (ua * ua + r * r).sqrt();
    let beta = r.atan2(rb + ua);
    let theta_max = |rho: f64| {
        if rho >= rho_w {
            (ua / rho).min(1.0).asin()
        } else {
            let s = (d * d - rb * rb + rho * rho) / (2.0 * rho * d);
            s.min(1.0).asin() - beta
        }
    };
    let f = |phi: f64| {
        let rho = big_r - r * phi.cos();
        rho * r * r * (1.0 - phi.sin()) * phi.sin() * theta_max(rho)
    };
    let phi_w = ((big_r - rho_w) / r).acos();
    let both = |a: f64, b: f64| {
        let x = gauss(&f, a, b, 20, 8);
        let y = gauss(&f, a, b, 30, 16);
        assert!((x - y).abs() <= 1e-14 * x.abs(), "{x} {y}");
        0.5 * (x + y)
    };
    let material = both(0.0, phi_w) + both(phi_w, PI / 2.0);
    -9.0 * a1 + (6.0 - ua) * strip + material
}

/// The kernel case `10876-fillet-ledge-convex-and-concave-edges-at-bore-r0-5` (#10876): the
/// ledge's convex edge (top and bore bottom) and concave edge (top and arch front) rounded r
/// 0.5 together, at the corner beside the bore's tangent seam. It builds as the blend corner's
/// Roll-over: the concave round's End at the cliff spans the bore's round bottom and its flat
/// wall across their tangent seam, its rib in two pieces through the seam's line. The result
/// is valid at Level 2, one closed manifold solid facing out, and its volume is the operand's
/// closed form plus [`ledge_rolled_change`] within 1e-11 relative (the End cut on the
/// cylinder continued instead of the wall would move it 7.2e-6, 1.6e-10 relative), which the
/// kernel's certified enclosure holds; the cap's two faces meet along the seam's line from W
/// to S, at its closed-form point, where the rib's two pieces meet.
///
/// The enclosures are asked 1e-3 wide, not the 1e-5 of the stair twin: two of the operand's
/// own edges, far from the corner (the radius-18 cylinder's at y = -16.36, z = 6, x from -7.5
/// to 7.5, and the radius-12 bore's at y = 27, z = 25.06, x from -9 to 9), leave gaps between
/// their faces whose strips settle the operand's enclosure at 7.4e-4 before any blend
/// (measured; the blend's two rib pieces add 8e-7). 1e-3 still decides the Roll-over (the
/// torus's material alone adds 0.032); the split at the seam is decided by the volume to
/// 1e-11 and the seam edge, not by the enclosure.
#[test]
fn the_ledge_corner_rolls_over_across_the_bores_tangent_seam() {
    rig_fixes();
    let b = ledge();
    let v0 = ledge_operand_volume();
    assert!(rel(b.volume(1e-12).unwrap(), v0) < 1e-11, "{v0}");
    enclosure_holds(&b, v0, 1e-3);
    let e1 = nearest_edge(&b, [12.0, 25.5, 33.0]);
    let e2 = nearest_edge(&b, [15.0, 30.0, 33.0]);
    blend(
        &b,
        Blend::Round,
        &[e1, e2],
        &[0.5, 0.5],
        &[],
        Corners::Filled,
    )
    .unwrap();
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert!(b.is_manifold_solid().unwrap());
    b.check_solid().unwrap();
    // the app's record holds it (each pcurve runs over its edge's interval), as built and
    // after a round trip through a .smb file (the case's render reads it back)
    b.to_record().unwrap();
    let back = Body::from_smb(b.to_smb(true).unwrap(), true).unwrap();
    back.to_record().unwrap();
    let want = v0 + ledge_rolled_change(0.5);
    // the volume and the seam, on the body as built and as read back from the .smb file
    for (which, body) in [("built", &b), ("reloaded", &back)] {
        let v = body.volume(1e-12).unwrap();
        assert!(rel(v, want) < 1e-11, "{which}: {v} {want}");
        enclosure_holds(body, want, 1e-3);
        // the cap's two faces meet along the seam's line from W to S, where the rib's two pieces
        // meet: S on that line (x = 12, z = 33) at the torus's height h = sqrt((rho - r)(3r - rho))
        // over its centre plane y = 30 - r, rho = sqrt(ua^2 + r^2) W's distance from A1
        let r: f64 = 0.5;
        let ua = ((12.0 + r) * (12.0 + r) - r * r).sqrt() - 12.0;
        let rho = (ua * ua + r * r).sqrt();
        let h = ((rho - r) * (3.0 * r - rho)).sqrt();
        let (w, s) = ([12.0, 30.0, 33.0], [12.0, 30.0 - r + h, 33.0]);
        let (_, brep) = kernel_copy(body);
        let near = |p: Vec3, q: [f64; 3]| (p - Vec3::new(q[0], q[1], q[2])).length() <= 1e-9;
        let ends = |e: EdgeId| {
            let v0 = edge::start_vertex(&brep, e).unwrap().unwrap();
            let v1 = edge::end_vertex(&brep, e).unwrap().unwrap();
            (
                brep.vertex(v0).unwrap().point,
                brep.vertex(v1).unwrap().point,
            )
        };
        let seam: Vec<EdgeId> = brep
            .edges()
            .ids()
            .filter(|&e| {
                let (p, q) = ends(e);
                (near(p, w) && near(q, s)) || (near(p, s) && near(q, w))
            })
            .collect();
        assert_eq!(seam.len(), 1, "{which}: the cap's seam edge from W to S");
        let faces = edge::faces(&brep, seam[0], None).unwrap();
        assert!(
            faces.len() == 2 && faces[0] != faces[1],
            "{which}: {faces:?}"
        );
        let at_s = brep
            .edges()
            .ids()
            .filter(|&e| {
                let (p, q) = ends(e);
                near(p, s) || near(q, s)
            })
            .count();
        assert_eq!(
            at_s, 3,
            "{which}: S, the seam edge and the rib's two pieces"
        );
    }
}

/// #18671: the ledge's operand asked 2.5e-4 wide, below the 7.4e-4 its strips settle at
/// ([`the_ledge_corner_rolls_over_across_the_bores_tangent_seam`]), ends at its floor: the
/// enclosure's least width (the strips' least widths however they are cut) is past 2.5e-4, so no
/// refinement reaches it, and the enclosure is within 2.5e-4 of that least width, so no refinement
/// narrows it by more than the width asked. It still holds the operand's closed-form volume, and it
/// takes well under a quarter of the cell budget: before, it spent all of [`CELL_BUDGET`] (131,375
/// cells, 129,094 of them strip cells) for 7.43e-4. A looser ask on the other side of the least
/// width is still reached, and its answer bounds how much wider the Floor answer can be.
#[test]
fn the_ledge_operand_asked_below_its_strips_ends_at_its_floor() {
    use solidcore::mass::enclosure::{enclose_volume, EnclosureStop, CELL_BUDGET};
    rig_fixes();
    let b = ledge();
    let v0 = ledge_operand_volume();
    let (k, mut brep) = kernel_copy(&b);
    let e = enclose_volume(&k, &mut brep, 2.5e-4).unwrap();
    assert_eq!(e.stop, EnclosureStop::Floor, "{e:?}");
    assert!(e.least > 2.5e-4, "{e:?}");
    assert!(
        e.least <= e.width() && e.width() - e.least <= 2.5e-4,
        "{e:?}"
    );
    // and no wider than the 1e-3 the ledge's own test asks and reaches
    assert!(e.width() <= 1e-3, "{e:?}");
    assert!(e.lo <= v0 && v0 <= e.hi, "{e:?} {v0}");
    assert!(e.counts.cells < CELL_BUDGET / 4, "{e:?}");

    // #21588 review: asked just below the Floor answer (7.456e-4, against 7.4638e-4 at least
    // 5.09e-4), past that least width, refinement goes on and reaches it (in 23,191 cells), so
    // the looser ask is answered narrower. That is the contract ([`enclose_volume`]): a rule that
    // kept the answers monotone would have to end this reachable ask unreached as well, or refine
    // 2.5e-4 on to the whole budget. The last two asserts hold by construction whatever the cells
    // become: the looser answer, as every answer, is at least the Floor answer's least width wide,
    // so the Floor answer is at most the width it asked wider. The Floor stop above and the reach
    // here depend on the measured numbers (the Floor answer is 2.37e-4 off its least width against
    // the 2.5e-4 asked, and the strips settle at 7.43e-4 given the whole budget).
    let a = e.width() * (1.0 - 1e-3);
    assert!(e.least <= a && a < e.width(), "{e:?}");
    let (k, mut brep) = kernel_copy(&b);
    let loose = enclose_volume(&k, &mut brep, a).unwrap();
    assert_eq!(loose.stop, EnclosureStop::Reached, "{loose:?}");
    assert!(loose.width() <= a, "{loose:?}");
    assert!(loose.lo <= v0 && v0 <= loose.hi, "{loose:?} {v0}");
    assert!(loose.width() >= e.least, "{loose:?} {e:?}");
    assert!(e.width() - loose.width() <= 2.5e-4, "{loose:?} {e:?}");
}

/// The ledge's planar twin (#10876): the slab [0, 10] x [-10, 10] x [-10, 0] with the riser
/// [0, 10] x [0, 10] x [0, 10] on it. Their walls x = 0 are one face, so three edges meet at
/// V = 0: E1 (the top z = 0 and the wall, convex), E2 (the top and the riser's front y = 0,
/// concave) and the wall's edge with the riser's front (convex).
fn stair_twin() -> Body {
    let slab = make_box([0.0, -10.0, -10.0], [10.0, 20.0, 10.0]).unwrap();
    let riser = make_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap();
    solidcore_ops::boolean::boolean(slab, riser, solidcore_ops::BooleanOp::Union)
        .unwrap()
        .unwrap()
}

/// The `n` Gauss-Legendre nodes and weights on [-1, 1]: each node the root of the Legendre
/// polynomial `P_n` from its Chebyshev estimate by Newton's method (the three-term recurrence
/// for `P_n` and its derivative), to machine precision.
fn gauss_legendre(n: usize) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for i in 1..=n {
        let mut x = (PI * (i as f64 - 0.25) / (n as f64 + 0.5)).cos();
        let mut dp = 1.0;
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, x);
            for j in 2..=n {
                let p2 = ((2 * j - 1) as f64 * x * p1 - (j - 1) as f64 * p0) / j as f64;
                p0 = p1;
                p1 = p2;
            }
            dp = n as f64 * (x * p1 - p0) / (x * x - 1.0);
            let step = p1 / dp;
            x -= step;
            if step.abs() < 1e-16 {
                break;
            }
        }
        out.push((x, 2.0 / ((1.0 - x * x) * dp * dp)));
    }
    out
}

/// `f` integrated over `[a, b]` by the `n`-point Gauss-Legendre rule on `panels` equal panels.
fn gauss(f: &dyn Fn(f64) -> f64, a: f64, b: f64, n: usize, panels: usize) -> f64 {
    let rule = gauss_legendre(n);
    let h = (b - a) / panels as f64;
    let mut sum = 0.0;
    for k in 0..panels {
        let (lo, hi) = (a + h * k as f64, a + h * (k + 1) as f64);
        for &(x, w) in &rule {
            sum += w * 0.5 * (hi - lo) * f(0.5 * (lo + hi) + 0.5 * (hi - lo) * x);
        }
    }
    sum
}

/// The material the blend corner's Roll-over adds on the stair twin (E1 rounded `r1`, E2
/// `r2`), past E2's fillet strip: about E1's axis A1 (x = r1, z = -r1, along y), the concave
/// ball's region for each direction `theta` from +z towards -x over the quarter turn the round
/// spans, in the meridian (distance `rho` from A1, height along y) the fillet section between
/// the round (`rho = r1`), the riser's front (y = 0) and the ball (centre `rho = R = r1 + r2`,
/// y = -r2): of height `r2 - sqrt(r2^2 - (R - rho)^2)` at `rho`. The cliff x = 0 keeps
/// `rho sin(theta) <= r1`, so `theta` up to `asin(r1 / rho)`:
/// `I = int_r1^R rho (r2 - sqrt(r2^2 - (R - rho)^2)) asin(r1 / rho) d rho`, and with
/// `rho = R - r2 cos(phi)` (analytic in `phi` over `[0, pi/2]`)
/// `I = int_0^(pi/2) rho r2^2 (1 - sin phi) sin phi asin(r1 / rho) d phi`. Integrated by two
/// Gauss-Legendre rules (20 points on 8 panels, 30 points on 16) that agree to 1e-14
/// relative (asserted; measured 9e-16, the rounding of summing their 160 and 480 terms), a
/// thousand times under the volume tests' 1e-11: the reference is their mean.
fn roll_over_material(r1: f64, r2: f64) -> f64 {
    let big_r = r1 + r2;
    let f = |phi: f64| {
        let rho = big_r - r2 * phi.cos();
        rho * r2 * r2 * (1.0 - phi.sin()) * phi.sin() * (r1 / rho).min(1.0).asin()
    };
    let a = gauss(&f, 0.0, PI / 2.0, 20, 8);
    let b = gauss(&f, 0.0, PI / 2.0, 30, 16);
    assert!((a - b).abs() <= 1e-14 * a.abs(), "{a} {b}");
    0.5 * (a + b)
}

/// The stair twin's volume once E1 is rounded `r1` and E2 `r2` as the blend corner's
/// Roll-over: 3000, less E1's strip `(1 - pi/4) r1^2` over its length 10, plus E2's fillet
/// strip `(1 - pi/4) r2^2` over its remainder from x = r1 to 10, plus the material the trimmed
/// torus adds ([`roll_over_material`]). At r1 = r2 = 1: 3000.047071660769.
fn rolled_volume(r1: f64, r2: f64) -> f64 {
    let strip = 1.0 - PI / 4.0;
    3000.0 - 10.0 * strip * r1 * r1 + (10.0 - r1) * strip * r2 * r2 + roll_over_material(r1, r2)
}

/// The kernel's certified enclosure of `b`'s volume holds `want`, and is narrower than
/// `width` (so that it decides `want` against a nearby value). The enclosure is asked for
/// `width` itself: refinement stops as soon as it is that narrow, and a tighter ask only
/// refines further, past what is asserted (on the ledge, into strips that settle at 7.4e-4
/// and spend the cell budget).
fn enclosure_holds(b: &Body, want: f64, width: f64) {
    let (k, mut brep) = kernel_copy(b);
    let e = solidcore::mass::enclosure::enclose_volume(&k, &mut brep, width).unwrap();
    assert!(e.lo <= want && want <= e.hi, "{} {want} {}", e.lo, e.hi);
    assert!(e.hi - e.lo <= width, "{} {}", e.lo, e.hi);
}

/// The torus face's edges of `brep` are exactly `ends`, each the two points its edge runs
/// between (either way round, each within 1e-9) and the kind of the face beside it.
fn torus_edges_are(brep: &solidcore::Brep, torus: FaceId, ends: &[([f64; 3], [f64; 3], &str)]) {
    let edges = solidcore::topology::face::p4::edges(brep, torus, None, false, None).unwrap();
    assert_eq!(edges.len(), ends.len(), "the torus face's edges");
    let near = |p: Vec3, q: [f64; 3]| (p - Vec3::new(q[0], q[1], q[2])).length() <= 1e-9;
    for &(a, b, kind) in ends {
        let found = edges.iter().any(|&e| {
            let v0 = edge::start_vertex(brep, e).unwrap().unwrap();
            let v1 = edge::end_vertex(brep, e).unwrap().unwrap();
            let (p, q) = (
                brep.vertex(v0).unwrap().point,
                brep.vertex(v1).unwrap().point,
            );
            let faces = edge::faces(brep, e, None).unwrap();
            let other = *faces.iter().find(|&&f| f != torus).unwrap();
            let beside = surface_kind_name(brep.face(other).unwrap().surface.as_ref().unwrap());
            ((near(p, a) && near(q, b)) || (near(p, b) && near(q, a))) && beside == kind
        });
        assert!(found, "no torus edge {a:?} - {b:?} beside a {kind}");
    }
}

/// The twin's convex edge E1 rounded at r = 1 alone (it ends on the riser's front continued,
/// its end cap a spandrel on that plane), then the concave chain it leaves, E2's remainder and
/// E1's end arc on the riser's front, rounded at r = 1 on its own: the blend corner's
/// Roll-over's second pass, asked as a feature of its own.
fn stair_twin_rolled() -> Body {
    rig_fixes();
    let b = stair_twin();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let chain = [
        nearest_edge(&b, [5.0, 0.0, 0.0]),
        nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
    ];
    assert_ne!(chain[0], chain[1]);
    blend(&b, Blend::Round, &chain, &[1.0], &[], Corners::Filled).unwrap();
    b
}

/// The twin's torus face, with its torus's centre, axis and radii: the one face on a torus
/// (an exact torus, or a surface of revolution of a circular arc, read off its arc; the test
/// below proves the face on that torus through its normals); its other faces planes and
/// cylinders.
fn rolled_torus(brep: &solidcore::Brep) -> (FaceId, (Vec3, Vec3, f64, f64)) {
    let mut tori = Vec::new();
    for &f in brep.face_list() {
        let surface = brep.face(f).unwrap().surface.as_ref().unwrap();
        if let Some(t) =
            solidcore::surface::procedural_blend::exact::revolved_circle_torus(surface).unwrap()
        {
            tori.push((f, t));
            continue;
        }
        match surface.kind() {
            SurfaceKind::Torus(t) => {
                let place = t.base().position();
                tori.push((
                    f,
                    (
                        place.origin(),
                        place.z_axis(),
                        t.major_radius(),
                        t.minor_radius(),
                    ),
                ));
            }
            SurfaceKind::Plane(_) => {}
            SurfaceKind::Cone(c) => assert!(c.is_cylinder(), "a face on a cone"),
            _ => panic!("a face on no exact surface: {}", surface_kind_name(surface)),
        }
    }
    assert_eq!(tori.len(), 1, "one torus face");
    tori[0]
}

/// The exact unit normal out of the rolled twin at `p` on its torus (towards the concave
/// ball's centre, which lies on the circle of radius 2 about E1's axis x = 1, z = -1, in the
/// plane y = -1), with `p`'s distance `rho` from that axis.
fn torus_normal(p: Vec3) -> (Vec3, f64) {
    let (dx, dz) = (p.x - 1.0, p.z + 1.0);
    let rho = (dx * dx + dz * dz).sqrt();
    let centre = Vec3::new(1.0 + 2.0 * dx / rho, -1.0, -1.0 + 2.0 * dz / rho);
    ((centre - p).unitize().unwrap(), rho)
}

#[test]
fn a_stair_rounded_convex_first_rolls_its_concave_chain_over_the_round() {
    let b = stair_twin_rolled();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let v = b.volume(1e-12).unwrap();
    let want = rolled_volume(1.0, 1.0);
    assert!(rel(v, want) < 1e-11, "{v}");
    enclosure_holds(&b, want, 1e-5);
    let (k, mut brep) = kernel_copy(&b);
    let (torus, (o, z, major, minor)) = rolled_torus(&brep);
    assert!(
        (major - 2.0).abs() < 1e-12 && (minor - 1.0).abs() < 1e-12,
        "{major} {minor}"
    );
    assert!(
        (o.x - 1.0).abs() < 1e-12 && (o.z + 1.0).abs() < 1e-12,
        "{o:?}"
    );
    assert!(z.x.abs() < 1e-12 && z.z.abs() < 1e-12, "{z:?}");
    // its four edges: the crease with the wall x = 0, from where the rail leaves the riser's
    // front (rho = 2: z = sqrt 3 - 1) to the tangent point on the round (rho = 1, in the balls'
    // centres' plane y = -1); the rail on the riser's front, the circle rho = 2 at y = 0 from
    // there to E2's fillet (1, 0, 1); the seam with E2's fillet in the plane x = 1; and the
    // seam with E1's round in the balls' centres' plane y = -1
    let lift = 3f64.sqrt() - 1.0;
    torus_edges_are(
        &brep,
        torus,
        &[
            ([0.0, 0.0, lift], [0.0, -1.0, -1.0], "plane"),
            ([0.0, 0.0, lift], [1.0, 0.0, 1.0], "plane"),
            ([1.0, -1.0, 0.0], [1.0, 0.0, 1.0], "cone"),
            ([0.0, -1.0, -1.0], [1.0, -1.0, 0.0], "cone"),
        ],
    );
    for e in solidcore::topology::face::p4::edges(&brep, torus, None, false, None).unwrap() {
        let faces = edge::faces(&brep, e, None).unwrap();
        let other = *faces.iter().find(|&&f| f != torus).unwrap();
        let wall = match brep.face(other).unwrap().surface.as_ref().unwrap().kind() {
            SurfaceKind::Plane(p) => p.is_planar().1.x.abs() > 1.0 - 1e-12,
            _ => false,
        };
        let ivl = brep.edge(e).unwrap().interval;
        for i in 1..8 {
            let s = ivl.min() + (ivl.max() - ivl.min()) * f64::from(i) / 8.0;
            let frame = |f: FaceId, brep: &mut solidcore::Brep| {
                let eu = edge::edgeuse_of_face(brep, e, Some(f)).unwrap().unwrap();
                let fu = solidcore::topology::face::p2::outer_faceuse(brep, f)
                    .unwrap()
                    .unwrap();
                let frame =
                    solidcore::query::api_queries::edgeuse_face_frame(&k, brep, eu, s, fu).unwrap();
                let at = brep
                    .face(f)
                    .unwrap()
                    .surface
                    .as_ref()
                    .unwrap()
                    .evaluate_point(&k, frame.uv)
                    .unwrap();
                (frame.normal, at)
            };
            // the exact normal where the torus face's normal is taken: its surface's point at
            // the edge use's (u, v)
            let (on_torus, p) = frame(torus, &mut brep);
            let (exact, rho) = torus_normal(p);
            assert!(
                (on_torus.value - exact).length() <= 1e-9 + on_torus.bound,
                "{p:?} {:?} {exact:?} {} wall {wall}",
                on_torus.value,
                on_torus.bound
            );
            if wall {
                let cos_p = 2.0 - rho;
                let cos_a = on_torus.value.x.abs();
                assert!((cos_a - cos_p / (2.0 - cos_p)).abs() <= 1e-9, "{p:?}");
            } else {
                let (beside, q) = frame(other, &mut brep);
                // the face beside meets the torus where the edge lies: the same exact normal,
                // to the edge's own gap between its two faces' points
                let gap = (q - p).length();
                assert!(
                    (beside.value - exact).length() <= 1e-9 + beside.bound + gap,
                    "{p:?}"
                );
            }
        }
    }
}

/// A nearby corner that is not the Roll-over still refuses as before: the riser runs on past
/// the wall over the void (the riser [-5, 10] x [0, 10] x [0, 10] on the slab), so that at V
/// the edge beyond the seam is the riser's front on its underside (no cliff rim) and the slab's
/// wall meets the riser's underside at a concave edge (no tangent seam). The vertex has four
/// edges, a convex and a concave round meeting there, and is refused as before.
#[test]
fn a_stair_whose_riser_overhangs_the_wall_still_refuses_its_corner() {
    rig_fixes();
    let slab = make_box([0.0, -10.0, -10.0], [10.0, 20.0, 10.0]).unwrap();
    let riser = make_box([-5.0, 0.0, 0.0], [15.0, 10.0, 10.0]).unwrap();
    let b = solidcore_ops::boolean::boolean(slab, riser, solidcore_ops::BooleanOp::Union)
        .unwrap()
        .unwrap();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    let e2 = nearest_edge(&b, [5.0, 0.0, 0.0]);
    let got = blend(&b, Blend::Round, &[e1, e2], &[1.0], &[], Corners::Filled);
    assert!(
        matches!(&got, Err(Error::Unsupported(r)) if r.contains("vertex of four edges")),
        "{:?}",
        got.err()
    );
}

/// The stair twin with its wall split along z = 0 under the riser (the wall imprinted with the
/// box [-5, 0] x [0, 10] x [0, 10]): four edges meet at V = 0, as at the ledge's corner (#10876):
/// E1 (the top and the slab's wall, convex), E2 (the top and the riser's front, concave), the
/// tangent seam between the two walls, and the riser's wall's edge with its front (the cliff's
/// rim, sharp and convex).
fn stair_twin_of_four_edges() -> Body {
    let tool = make_box([-5.0, 0.0, 0.0], [5.0, 10.0, 10.0]).unwrap();
    let (b, changed) = solidcore_ops::boolean::imprint(stair_twin(), tool).unwrap();
    assert!(changed);
    let (record, _) = b.to_record().unwrap();
    Body::from_record(&record, true).unwrap().0
}

/// The four-edge twin's E1 rounded at 1 and E2 at 0.4 in one call: the corners step refuses
/// V as a convex and a concave round meeting at four edges, notes it as the blend corner's
/// Roll-over, and builds it in two passes (E1 first, then E2's remainder with E1's end arc on
/// the riser's front as one concave chain). At r2 = 0.4 the torus's rail on the riser's front
/// (rho = R = 1.4 about E1's axis) leaves it across the slab's wall below V (at
/// z = sqrt(R^2 - 1) - 1 < 0), so the End lies on one cliff face. The result is valid
/// (level 2), its volume is the closed form's (the certified enclosure holds it), its one
/// torus has radii 1.4 and 0.4 about E1's axis, and its four edges are the crease with the
/// wall, the rail on the riser's front and the seams with the two rounds.
#[test]
fn a_ledge_twin_of_four_edges_rolls_over_in_one_call() {
    rig_fixes();
    let (r1, r2) = (1.0, 0.4);
    let b = stair_twin_of_four_edges();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    let e2 = nearest_edge(&b, [5.0, 0.0, 0.0]);
    blend(&b, Blend::Round, &[e1, e2], &[r1, r2], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let want = rolled_volume(r1, r2);
    let v = b.volume(1e-12).unwrap();
    assert!(rel(v, want) < 1e-11, "{v} {want}");
    enclosure_holds(&b, want, 1e-5);
    let (_, brep) = kernel_copy(&b);
    let (torus, (o, z, major, minor)) = rolled_torus(&brep);
    assert!(
        (major - (r1 + r2)).abs() < 1e-12 && (minor - r2).abs() < 1e-12,
        "{major} {minor}"
    );
    assert!(
        (o.x - r1).abs() < 1e-12 && (o.y + r2).abs() < 1e-12 && (o.z + r1).abs() < 1e-12,
        "{o:?}"
    );
    assert!(z.x.abs() < 1e-12 && z.z.abs() < 1e-12, "{z:?}");
    let exit = ((r1 + r2) * (r1 + r2) - r1 * r1).sqrt() - r1;
    torus_edges_are(
        &brep,
        torus,
        &[
            ([0.0, 0.0, exit], [0.0, -r2, -r1], "plane"),
            ([0.0, 0.0, exit], [r1, 0.0, r2], "plane"),
            ([r1, -r2, 0.0], [r1, 0.0, r2], "cone"),
            ([0.0, -r2, -r1], [r1, -r2, 0.0], "cone"),
        ],
    );
}

/// The four-edge twin rounded at 1 and 1 in one call: the corner is the Roll-over's, but its
/// two passes do not build (the rail on the riser's front leaves it across the riser's wall,
/// above V, and the End's rib would span both cliff faces), so the call falls back to the
/// first call's refusal, the corner's four-edge reason, and leaves the body as it was.
#[test]
fn a_ledge_twin_whose_roll_over_does_not_build_keeps_its_refusal() {
    rig_fixes();
    let b = stair_twin_of_four_edges();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    let e2 = nearest_edge(&b, [5.0, 0.0, 0.0]);
    let got = blend(&b, Blend::Round, &[e1, e2], &[1.0], &[], Corners::Filled);
    assert!(
        matches!(&got, Err(Error::Unsupported(r)) if r.contains("vertex of four edges")),
        "{:?}",
        got.err()
    );
    assert!(rel(b.volume(1e-12).unwrap(), 3000.0) < 1e-12);
}

/// #17617's plate and wall, turned so that the wall's profile lies in z = 0: the plate
/// [29.5, 138.5] x [-4, 0] x [-33, 117] and on its face y = -4 the wall, `profile` swept 5
/// along z.
fn plate_and_wall(profile: &[(f64, f64)]) -> Body {
    let plate = make_box([29.5, -4.0, -33.0], [109.0, 4.0, 150.0]).unwrap();
    let sides: Vec<_> = (0..profile.len())
        .map(|i| segment(profile[i], profile[(i + 1) % profile.len()]))
        .collect();
    let wall = swept(&sides, 5.0);
    solidcore_ops::boolean::boolean(plate, wall, solidcore_ops::BooleanOp::Union)
        .unwrap()
        .unwrap()
}

/// A round of 2 on the concave edge E between the plate's face y = -4 and the wall's face
/// z = 5 (#17617). E runs from x = 129.5, where the wall's end is square, to V = (29.5, -4, 5),
/// the plate's end, where the wall's end face W slants out past the plate (the plane through
/// x = 29.5, y = -4 and x = 10, y = -40). At V four edges meet: E, the plate's corner edge
/// x = 29.5, y = -4 (convex) and W's two edges with the wall's face z = 5 (convex) and with the
/// plate's end x = 29.5. The rails end on the two convex edges, at (29.5, -4, 7) and at
/// (29.5 - 2 * 19.5 / 36, -6, 5), so the round ends on W's plane and W grows over the end's
/// cap (it held no cap on the plate's end plane, and the round was refused as no closed
/// solid). The round's cross-section is the square of side 2 less the ball's quarter disc,
/// its length 100 plus how far W slants back over that section. The wall's other edge with the
/// plate (its face z = 0) is the mirror image, its side edges the other way round V.
#[test]
#[allow(clippy::disallowed_methods)] // the test's closed form, not ops computing geometry
fn a_round_ending_where_a_slanted_wall_overhangs_its_plate_ends_on_the_wall() {
    rig_fixes();
    // first, on both of the wall's sides, the same wall with its end square, overhanging the
    // plate's end by 19.5: the round ends on the plate's end plane (as before #17617)
    for z in [5.0, 0.0] {
        let b = plate_and_wall(&[(10.0, -4.0), (129.5, -4.0), (129.5, -40.0), (10.0, -40.0)]);
        let e = nearest_edge(&b, [80.0, -4.0, z]);
        blend(&b, Blend::Round, &[e], &[2.0], &[], Corners::Filled).unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "z = {z}");
        assert_eq!(b.counts().unwrap(), [13, 32, 21], "z = {z}");
        let v = b.volume(1e-12).unwrap();
        let want = 65400.0 + 119.5 * 36.0 * 5.0 + 100.0 * (4.0 - PI);
        assert!(rel(v, want) < 1e-11, "z = {z}: {v}");
    }
    let slant = 19.5 / 36.0;
    let before = 65400.0 + 0.5 * (100.0 + 119.5) * 36.0 * 5.0;
    // the wall's face z and the side of it the round lies on
    for (z, out) in [(5.0, 1.0), (0.0, -1.0)] {
        let b = plate_and_wall(&[(29.5, -4.0), (129.5, -4.0), (129.5, -40.0), (10.0, -40.0)]);
        assert_eq!(b.counts().unwrap(), [11, 25, 16]);
        assert!(rel(volume(&b), before) < 1e-12);
        let e = nearest_edge(&b, [80.0, -4.0, z]);
        blend(&b, Blend::Round, &[e], &[2.0], &[], Corners::Filled).unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "z = {z}");
        assert_eq!(b.counts().unwrap(), [12, 28, 18], "z = {z}");
        // the section's area 4 - pi, its first moment about the plate's face 20/3 - 2 pi
        let want = before + 100.0 * (4.0 - PI) + slant * (20.0 / 3.0 - 2.0 * PI);
        let v = b.volume(1e-12).unwrap();
        assert!(rel(v, want) < 1e-11, "z = {z}: {v} {want}");
        let (k, brep) = kernel_copy(&b);
        let near = |a: Vec3, p: [f64; 3]| (a - Vec3::new(p[0], p[1], p[2])).length() < 1e-9;
        let top = z + 2.0 * out;
        let (mut rounds, mut walls) = (0, 0);
        for &f in brep.face_list() {
            let bb = face_bounding_box(&k, &brep, f, true).unwrap();
            match brep.face(f).unwrap().surface.as_ref().unwrap().kind() {
                SurfaceKind::Cone(c) => {
                    assert!(c.is_cylinder(), "z = {z}: the round on a cone");
                    let lo = [29.5 - 2.0 * slant, -6.0, z.min(top)];
                    assert!(near(bb.min(), lo), "z = {z}: {bb:?}");
                    assert!(near(bb.max(), [129.5, -4.0, z.max(top)]), "z = {z}: {bb:?}");
                    rounds += 1;
                }
                SurfaceKind::Plane(_) => {
                    // W, out to the cap's far side
                    let (lo, hi) = ([10.0, -40.0, top.min(0.0)], [29.5, -4.0, top.max(5.0)]);
                    if near(bb.min(), lo) && near(bb.max(), hi) {
                        walls += 1;
                    }
                }
                _ => panic!(
                    "a face on {}",
                    surface_kind_name(brep.face(f).unwrap().surface.as_ref().unwrap())
                ),
            }
        }
        assert_eq!((rounds, walls), (1, 1), "z = {z}");
    }
}

/// A block 30 x 30 x 10 with the pocket [5, 25]^2 x [4, 10], its four vertical edges rounded
/// `RC`: its floor's edges are four lines and four arcs, each concave, each arc between a
/// corner's cylinder and the floor (square to the cylinder's axis), each meeting the next
/// edge G1.
const RC: f64 = 8.0;
fn rounded_pocket() -> Body {
    let block = make_box([0.0, 0.0, 0.0], [30.0, 30.0, 10.0]).unwrap();
    let cut = make_box([5.0, 5.0, 4.0], [20.0, 20.0, 10.0]).unwrap();
    let b = solidcore_ops::boolean::boolean(block, cut, solidcore_ops::BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let verticals: Vec<usize> = [[5.0, 5.0], [25.0, 5.0], [25.0, 25.0], [5.0, 25.0]]
        .iter()
        .map(|p| nearest_edge(&b, [p[0], p[1], 7.0]))
        .collect();
    blend(&b, Blend::Round, &verticals, &[RC], &[], Corners::Filled).unwrap();
    b
}

/// The rounded pocket's floor loop: its four lines, then its four arcs.
fn pocket_floor(b: &Body) -> Vec<usize> {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let mut out = Vec::new();
    for p in [[15.0, 5.0], [25.0, 15.0], [15.0, 25.0], [5.0, 15.0]] {
        out.push(nearest_edge(b, [p[0], p[1], 4.0]));
    }
    for (cx, cy, dx, dy) in [
        (5.0 + RC, 5.0 + RC, -1.0, -1.0),
        (25.0 - RC, 5.0 + RC, 1.0, -1.0),
        (25.0 - RC, 25.0 - RC, 1.0, 1.0),
        (5.0 + RC, 25.0 - RC, -1.0, 1.0),
    ] {
        out.push(nearest_edge(b, [cx + dx * RC * s, cy + dy * RC * s, 4.0]));
    }
    out
}

/// A pocket floor with rounded vertical corners is not the Roll-over's concave chain: at each
/// end of a corner's arc on the floor, the floor's next edge runs on concave (G1 into the next
/// wall's foot), never sharp and convex as the ledge's cliff rim is. So the gate refuses its
/// loop and any part of it, while the stair twin's chain (E2's remainder with E1's end arc on
/// the riser's front) passes it with its one V, the arc's end on the wall at (0, 0, -1).
#[test]
fn only_the_ledges_chain_is_the_roll_overs_second_pass() {
    use solidcore::blend::blend_corner::rollover::{concave_chain_ends, is_concave_chain};
    rig_fixes();
    let b = rounded_pocket();
    let floor = pocket_floor(&b);
    let (k, mut brep) = kernel_copy(&b);
    let ids = |brep: &solidcore::Brep, picks: &[usize]| -> Vec<EdgeId> {
        picks.iter().map(|&i| brep.edge_list()[i]).collect()
    };
    let all = ids(&brep, &floor);
    let part = ids(&brep, &[floor[0], floor[4], floor[5]]);
    for edges in [&all, &part] {
        assert!(!is_concave_chain(&k, &mut brep, edges).unwrap());
        assert!(concave_chain_ends(&k, &mut brep, edges).unwrap().is_empty());
    }
    let b = stair_twin();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let chain = [
        nearest_edge(&b, [5.0, 0.0, 0.0]),
        nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
    ];
    let (k, mut brep) = kernel_copy(&b);
    let chain = ids(&brep, &chain);
    assert!(is_concave_chain(&k, &mut brep, &chain).unwrap());
    let ends = concave_chain_ends(&k, &mut brep, &chain).unwrap();
    assert_eq!(ends.len(), 1);
    let p = brep.vertex(ends[0]).unwrap().point;
    assert!((p - Vec3::new(0.0, 0.0, -1.0)).length() < 1e-12, "{p:?}");
}

/// The rounded pocket's floor loop rounded at 1 is rounded as before (not as the Roll-over's
/// second pass): valid, and its volume the closed form's. The block less the pocket is
/// 9000 - 2400; each corner's round adds `(1 - pi/4) RC^2` over the wall's height 6; the
/// floor's round adds its section `A = (1 - pi/4) r^2` along the four lines (each
/// `20 - 2 RC` long) and, by Pappus, `A (pi / 2) (RC - c)` round each corner, `c` the section's
/// centroid's distance from the wall, `r (5/6 - pi/4) / (1 - pi/4)` (the square's moment less
/// the quarter disc's about the wall, over the area).
#[test]
fn a_pocket_floor_with_rounded_corners_is_rounded_as_before() {
    rig_fixes();
    let r = 1.0;
    let b = rounded_pocket();
    let floor = pocket_floor(&b);
    blend(&b, Blend::Round, &floor, &[r], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    let strip = 1.0 - PI / 4.0;
    let section = strip * r * r;
    let centroid = r * (5.0 / 6.0 - PI / 4.0) / strip;
    let want = 9000.0 - 2400.0
        + 4.0 * strip * RC * RC * 6.0
        + 4.0 * section * (20.0 - 2.0 * RC)
        + 4.0 * section * (PI / 2.0) * (RC - centroid);
    let v = b.volume(1e-12).unwrap();
    assert!(rel(v, want) < 1e-11, "{v} {want}");
}

/// The rounded pocket's floor loop rounded at 7, past the walls' height 6, is refused as
/// before: too large, its rail leaving the wall it rolls on over the pocket's rim (a concave
/// round adding material, never built as the Roll-over's trimmed cliff overflow), with a
/// largest size that builds under the walls' height; the body is left as it was.
#[test]
fn a_pocket_floor_round_past_its_walls_keeps_its_refusal() {
    rig_fixes();
    let b = rounded_pocket();
    let before = b.volume(1e-12).unwrap();
    let floor = pocket_floor(&b);
    let got = blend(&b, Blend::Round, &floor, &[7.0], &[], Corners::Filled);
    match got {
        Err(Error::TooLarge {
            size,
            kind: FitKind::Radius,
            why: Some(solidcore_ops::RoomReason::RailLeavesFace),
            ..
        }) => assert!(size > 5.9 && size < 6.0, "{size}"),
        other => panic!("not the too-large refusal: {other:?}"),
    }
    assert_eq!(b.volume(1e-12).unwrap().to_bits(), before.to_bits());
}

/// A plate of `tiles` 20 x 20 x 10 tiles side by side along x, each with its four top edges
/// rounded 5 before they are joined, and a wall standing on it: the box from (3, 3, 5) to
/// (20 tiles - 3, 17, 15) with its four upright edges rounded 1, joined on.
///
/// The rounds are mitred at a tile's sharp upright edges, so two of them meet in a ridge
/// at each corner (in the plane x = y at the origin's), and between two tiles the rounds
/// leave a valley down to z = 5 at x = 20. The wall's front (y = 3) meets the front rounds
/// (their axis y = 5, z = 5) along z = 5 + sqrt 21. Its foot crosses a ridge on each
/// corner arc of the wall and, with two tiles, where its front runs down into the valley
/// (at x = 17 and x = 23).
fn wall_on_rounded_tiles(tiles: usize) -> Body {
    let mut plate: Option<Body> = None;
    for i in 0..tiles {
        let x = 20.0 * i as f64;
        let tile = make_box([x, 0.0, 0.0], [20.0, 20.0, 10.0]).unwrap();
        let rim: Vec<usize> = [
            [x + 10.0, 0.0],
            [x + 10.0, 20.0],
            [x, 10.0],
            [x + 20.0, 10.0],
        ]
        .iter()
        .map(|p| nearest_edge(&tile, [p[0], p[1], 10.0]))
        .collect();
        blend(&tile, Blend::Round, &rim, &[5.0], &[], Corners::Filled).unwrap();
        plate = Some(match plate {
            None => tile,
            Some(joined) => {
                solidcore_ops::boolean::boolean(joined, tile, solidcore_ops::BooleanOp::Union)
                    .unwrap()
                    .unwrap()
            }
        });
    }
    let far = 20.0 * tiles as f64 - 3.0;
    let wall = make_box([3.0, 3.0, 5.0], [far - 3.0, 14.0, 10.0]).unwrap();
    let uprights: Vec<usize> = [[3.0, 3.0], [far, 3.0], [far, 17.0], [3.0, 17.0]]
        .iter()
        .map(|p| nearest_edge(&wall, [p[0], p[1], 10.0]))
        .collect();
    blend(&wall, Blend::Round, &uprights, &[1.0], &[], Corners::Filled).unwrap();
    solidcore_ops::boolean::boolean(plate.unwrap(), wall, solidcore_ops::BooleanOp::Union)
        .unwrap()
        .unwrap()
}

/// The refusal of a round that passes over the face it ends on, as `blend` gives it: its
/// size, which lies under `limit` by the fit margin, a thousand tolerances of the edge
/// `ending` whose round ends there.
fn passes_over_its_end_face<T: std::fmt::Debug>(
    b: &Body,
    got: Result<T, Error>,
    limit: f64,
    ending: usize,
) -> f64 {
    let (_, brep) = kernel_copy(b);
    let margin = 1000.0 * edge::tolerance(&brep, brep.edge_list()[ending]).unwrap();
    match got {
        Err(Error::TooLarge {
            size,
            kind: FitKind::Radius,
            why: Some(solidcore_ops::RoomReason::EndPassesOverEndFace),
            ..
        }) => {
            assert!(margin > 0.0 && margin < 0.1 * limit, "{margin}");
            assert!(
                (size - (limit - margin)).abs() < 1e-6,
                "{size} under {limit} by {margin}"
            );
            size
        }
        other => panic!("not the too-large refusal: {other:?}"),
    }
}

/// The foot of the wall's front, rounded as a tangent chain: the front's line and the two
/// corner arcs' halves it runs on into, each up to the ridge of the plate's corner. The round
/// ends there on the next plate round E (its axis x = 5, z = 5), which falls away under it:
/// E's surface crosses the wall's corner cylinder below the round's contact line, and that
/// line ends on E only where a ball touching the cylinder on that crossing also touches the
/// front round G. The crossing rises from the ridge to the corner arc's end, the tangent
/// line x = 4, y = 3, where it is 5 + sqrt 24 up. The ball touching the wall there, centre
/// (4, 3 - rho, 5 + sqrt 24), touches G where (2 + rho)^2 + 24 = (5 + rho)^2: rho = 1/2.
/// So a round under 1/2 builds, and one over it is refused as too large, naming the radius
/// that fits under 1/2, where it failed with a bare status before. At 1/2 itself the round's
/// end takes its whole corner arc and its edges fail: refused the same way. The body is left
/// as it was, and the size named builds a valid body.
#[test]
fn a_round_passing_over_the_face_it_ends_on_is_refused_naming_the_radius_that_fits() {
    rig_fixes();
    let b = wall_on_rounded_tiles(1);
    let before = b.volume(1e-12).unwrap();
    let foot = 5.0 + 21.0_f64.sqrt();
    // a corner arc's half, an eighth of a turn from the front's line about the corner's axis
    let (s, c) = (PI / 8.0).sin_cos();
    let on_arc = 5.0 + (25.0 - (1.0 + c) * (1.0 + c)).sqrt();
    let chain = [
        nearest_edge(&b, [10.0, 3.0, foot]),
        nearest_edge(&b, [4.0 - s, 4.0 - c, on_arc]),
        nearest_edge(&b, [16.0 + s, 4.0 - c, on_arc]),
    ];
    let mut fits = 0.0;
    for r in [1.0, 0.5] {
        let got = blend(&b, Blend::Round, &chain, &[r], &[], Corners::Filled);
        fits = passes_over_its_end_face(&b, got, 0.5, chain[1]);
        assert_eq!(b.volume(1e-12).unwrap().to_bits(), before.to_bits());
    }
    blend(&b, Blend::Round, &chain, &[fits], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    assert!(b.volume(1e-12).unwrap() > before);
}

/// On two tiles the wall's front runs down into the valley between them, and the line of its
/// foot on the second tile ends on that tile's ridge at x = 23, where the wall's front goes
/// on down the tile's valley round E (its axis x = 25, z = 5). Rounded alone, the line's
/// round ends there on E, which crosses the wall's front along z = 5 + sqrt(25 - (x - 25)^2):
/// highest, 10 up, at x = 25. The ball touching the front there, centre (25, 3 - rho, 10),
/// touches the front round where (2 + rho)^2 + 25 = (5 + rho)^2: rho = 2/3, the largest
/// along the crossing (an inner one). So a round over 2/3 is refused naming a radius under
/// 2/3, which builds. Rounded with the corner arc it runs on into at its other end, the limit
/// is that end's 1/2, and at 1/2 itself the arc's round has no extent, so its face fails:
/// refused the same way.
#[test]
fn a_round_ending_on_a_ridge_names_the_largest_ball_along_its_end_faces_crossing() {
    rig_fixes();
    let b = wall_on_rounded_tiles(2);
    let before = b.volume(1e-12).unwrap();
    let foot = 5.0 + 21.0_f64.sqrt();
    let line = nearest_edge(&b, [29.5, 3.0, foot]);
    let got = blend(&b, Blend::Round, &[line], &[1.0], &[], Corners::Filled);
    let fits = passes_over_its_end_face(&b, got, 2.0 / 3.0, line);
    assert_eq!(b.volume(1e-12).unwrap().to_bits(), before.to_bits());
    let (s, c) = (PI / 8.0).sin_cos();
    let on_arc = 5.0 + (25.0 - (1.0 + c) * (1.0 + c)).sqrt();
    let chain = [line, nearest_edge(&b, [36.0 + s, 4.0 - c, on_arc])];
    let got = blend(&b, Blend::Round, &chain, &[0.5], &[], Corners::Filled);
    passes_over_its_end_face(&b, got, 0.5, chain[1]);
    assert_eq!(b.volume(1e-12).unwrap().to_bits(), before.to_bits());
    blend(&b, Blend::Round, &[line], &[fits], &[], Corners::Filled).unwrap();
    assert!(b.check(CheckLevel::All).unwrap().is_valid());
    assert!(b.volume(1e-12).unwrap() > before);
}

/// The volume of the tetrahedron `v` with every edge rounded `r` and a ball at every corner:
/// the inner tetrahedron (`v` drawn in about its incentre until its faces are `r` nearer,
/// by `k = (rho - r) / rho`, rho the inradius `3 V / S`) with the ball of radius `r` rolled
/// all over it, by Steiner's formula `V k^3 + S k^2 r + M k r^2 + 4 pi r^3 / 3`, where
/// `M` is half the sum over the edges of each edge's length times the angle between its
/// faces' outward normals.
fn tetrahedron_rounded_all_over(v: &[Vec3; 4], r: f64) -> f64 {
    let centre = (v[0] + v[1] + v[2] + v[3]) * 0.25;
    let volume = (v[1] - v[0]).cross(v[2] - v[0]).dot(v[3] - v[0]).abs() / 6.0;
    let faces = [[0, 1, 2], [0, 1, 3], [1, 2, 3], [2, 0, 3]];
    let mut normals = Vec::new();
    let mut area = 0.0;
    for f in faces {
        let n = (v[f[1]] - v[f[0]]).cross(v[f[2]] - v[f[0]]);
        area += 0.5 * n.length();
        let n = n.unitize().unwrap();
        normals.push(if n.dot(v[f[0]] - centre) < 0.0 { -n } else { n });
    }
    let mut mean = 0.0;
    for i in 0..4 {
        for j in i + 1..4 {
            let shared: Vec<usize> = faces[i]
                .iter()
                .copied()
                .filter(|a| faces[j].contains(a))
                .collect();
            let turn = normals[i].dot(normals[j]).clamp(-1.0, 1.0).acos();
            mean += 0.5 * (v[shared[0]] - v[shared[1]]).length() * turn;
        }
    }
    let k = (3.0 * volume / area - r) / (3.0 * volume / area);
    volume * k.powi(3) + area * k * k * r + mean * k * r * r + 4.0 * PI * r.powi(3) / 3.0
}

/// A tetrahedron's corner is sharp: its three faces' normals stand far apart, and so do the
/// contacts of the ball that three equal rounds end on there. The Ball's face was one patch
/// of the sphere between those contacts, which holds only up to a regular tetrahedron's
/// corner (normals arccos(-1/3) apart). At a sharper one, and at the regular one itself, the
/// three rounds failed with "Corner Edge Computation Failure"; the triangle is now three
/// patches about the ball's point facing the vertex.
///
/// The tetrahedron on the triangle of circumradius 16.3 with its apex 16.33 above the
/// middle (its base corners' normals 116.5, 116.5 and 101.6 degrees apart, its apex's
/// 101.6 each, which was one patch already), and the regular one on the same base (apex
/// `16.3 sqrt 2` up): the three edges at a base corner build, and every edge rounded at 0.1
/// and at 2 gives the closed form of a solid rounded all over
/// (`tetrahedron_rounded_all_over`). Its faces are the 4 faces, the 6 rounds, and at each
/// corner one patch where the triangle is one (the first one's apex) and three where it is
/// not: 20 and 22.
#[test]
fn every_edge_of_a_tetrahedron_rounds_to_its_closed_form() {
    rig_fixes();
    let base: [[f64; 2]; 3] = [-30.0_f64, 90.0, 210.0].map(|a| {
        let a = a.to_radians();
        [16.3 * a.cos(), 16.3 * a.sin()]
    });
    for (height, faces) in [(16.33, 20), (16.3 * 2.0_f64.sqrt(), 22)] {
        let apex = [0.0, 0.0, height];
        let section: Vec<Curve> = (0..3)
            .map(|i| Curve::Line {
                start: [base[i][0], base[i][1], 0.0],
                end: [base[(i + 1) % 3][0], base[(i + 1) % 3][1], 0.0],
            })
            .collect();
        let tetrahedron = || loft_to_point(&section, apex).unwrap();
        let v = [
            Vec3::new(base[0][0], base[0][1], 0.0),
            Vec3::new(base[1][0], base[1][1], 0.0),
            Vec3::new(base[2][0], base[2][1], 0.0),
            Vec3::new(0.0, 0.0, height),
        ];
        let middle = |i: usize, j: usize| {
            let m = (v[i] + v[j]) * 0.5;
            [m.x, m.y, m.z]
        };
        // the three edges at the base corner 0: two base edges and the edge to the apex
        let b = tetrahedron();
        let corner = [(0, 1), (0, 2), (0, 3)].map(|(i, j)| nearest_edge(&b, middle(i, j)));
        blend(&b, Blend::Round, &corner, &[2.0], &[], Corners::Filled).unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid());
        for r in [0.1, 2.0] {
            let b = tetrahedron();
            let whole = b.volume(1e-12).unwrap();
            let sharp = (v[1] - v[0]).cross(v[2] - v[0]).dot(v[3] - v[0]).abs() / 6.0;
            assert!(rel(whole, sharp) < 1e-12, "{whole} {sharp}");
            let all = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)]
                .map(|(i, j)| nearest_edge(&b, middle(i, j)));
            blend(&b, Blend::Round, &all, &[r], &[], Corners::Filled).unwrap();
            assert!(b.check(CheckLevel::All).unwrap().is_valid());
            let want = tetrahedron_rounded_all_over(&v, r);
            let got = b.volume(1e-12).unwrap();
            assert!(
                rel(got, want) < 1e-11,
                "height {height}, r {r}: {got} {want}"
            );
            assert_eq!(b.counts().unwrap()[0], faces, "height {height}, r {r}");
        }
    }
}

/// In the Roll-over's second pass only the corner at V (where the concave chain meets the
/// cliff) looks for its rail ends across the cliff's tangent seam: on the stair twin's chain
/// (E1 rounded first), the corner at V = (0, 0, -1) carries the Roll-over's search and the
/// chain's other end, on the side face x = 10, does not (its rail ends are found as in any
/// pass, a rail end just past its side edge's end kept where the solve put it).
#[test]
fn only_the_corner_at_v_looks_across_the_cliffs_seam() {
    use solidcore::blend::blend_corner::rollover::concave_chain_ends;
    use solidcore::blend::fillet_executive::session::FilletSession;
    rig_fixes();
    let b = stair_twin();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let picks = [
        nearest_edge(&b, [5.0, 0.0, 0.0]),
        nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
    ];
    let (mut k, mut brep) = kernel_copy(&b);
    let chain: Vec<EdgeId> = picks.iter().map(|&i| brep.edge_list()[i]).collect();
    let ends = concave_chain_ends(&k, &mut brep, &chain).unwrap();
    assert_eq!(ends.len(), 1);
    let mut session = FilletSession::new(&mut k, &mut brep).unwrap();
    session
        .prepare_constant_radius_edges(&mut k, &chain, 1.0, 1, 1, 1.0)
        .unwrap();
    session.set_roll_over_pass(ends.clone());
    session.create_corners(&k, None).unwrap();
    let (mut at_v, mut far_end) = (0, 0);
    for id in session.fillet_corners().unwrap() {
        let corner = session.corner(id).unwrap();
        let vertex = corner.filleted_vertex().unwrap();
        assert_eq!(corner.roll_over, vertex == ends[0], "{vertex:?}");
        at_v += usize::from(vertex == ends[0]);
        far_end += usize::from(!corner.roll_over);
    }
    assert_eq!(at_v, 1, "a corner at V");
    assert!(far_end >= 1, "a corner away from V");
}

/// The stair twin's concave chain (E1 rounded at 1 first) rounded at 12, past the riser's
/// height 10: E2's remainder overflows the riser's top rim, away from V, so even as the
/// Roll-over's second pass (tried on the first call's refusal) that concave overflow is
/// refused (only a blend whose edge ends at V overflows as the event's). The call keeps the
/// first call's refusal, too large with its rail leaving the face, at a size under the riser's
/// height, and leaves the body as it was.
#[test]
fn an_oversized_ledge_chain_keeps_its_refusal() {
    rig_fixes();
    let b = stair_twin();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
    let before = b.volume(1e-12).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let chain = [
        nearest_edge(&b, [5.0, 0.0, 0.0]),
        nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
    ];
    let got = blend(&b, Blend::Round, &chain, &[12.0], &[], Corners::Filled);
    match got {
        Err(Error::TooLarge {
            size,
            kind: FitKind::Radius,
            why: Some(solidcore_ops::RoomReason::RailLeavesFace),
            ..
        }) => assert!(size > 0.0 && size < 10.0, "{size}"),
        other => panic!("not the too-large refusal: {other:?}"),
    }
    assert_eq!(b.volume(1e-12).unwrap().to_bits(), before.to_bits());
}

/// The stair twin with its riser notched beside the wall (the box [0, 0.95] x [0, 10] x
/// [0.9, 10] taken away): E1 rounded at 1 first, then its concave chain at 1. E2's remainder
/// (x >= 1) keeps its rail on the riser's front (z = 1 under the height 10), but E1's end arc
/// at V rolls its rail (the circle of radius 2 about E1's axis on y = 0) above the notch's
/// floor z = 0.9 for 0.38 < x < 0.95: it leaves the riser's front across the notch's edges,
/// away from the cliff's rim at V. That concave overflow is not the Roll-over's, so the second
/// pass refuses it and the call keeps the first call's refusal (too large, its rail leaving
/// the face), the body as it was.
#[test]
fn a_ledge_chain_whose_arc_overflows_away_from_the_cliff_keeps_its_refusal() {
    rig_fixes();
    let notch = make_box([0.0, 0.0, 0.9], [0.95, 10.0, 9.1]).unwrap();
    let b =
        solidcore_ops::boolean::boolean(stair_twin(), notch, solidcore_ops::BooleanOp::Subtract)
            .unwrap()
            .unwrap();
    let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
    blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
    let before = b.volume(1e-12).unwrap();
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let chain = [
        nearest_edge(&b, [5.0, 0.0, 0.0]),
        nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
    ];
    let got = blend(&b, Blend::Round, &chain, &[1.0], &[], Corners::Filled);
    match got {
        Err(Error::TooLarge {
            size,
            kind: FitKind::Radius,
            why: Some(solidcore_ops::RoomReason::RailLeavesFace),
            ..
        }) => assert!(size > 0.0 && size < 1.0, "{size}"),
        other => panic!("not the too-large refusal: {other:?}"),
    }
    assert_eq!(b.volume(1e-12).unwrap().to_bits(), before.to_bits());
}

/// The kernel's decision itself, in the Roll-over's second pass: the stair twin's chain
/// overflows the riser's front only across the cliff's rim at V, so its concave overflow is
/// the event's (the session builds); the notched twin's arc, though its edge ends at
/// V, leaves the front across the notch's edges, away from the rim, so its overflow is refused
/// as a `ConcaveOverflow`, as on main.
#[test]
fn only_an_overflow_across_the_cliffs_rim_at_v_is_the_roll_overs() {
    use solidcore::blend::blend_corner::rollover::concave_chain_ends;
    use solidcore::blend::fillet_executive::p3::NoRoomReason;
    use solidcore::blend::fillet_executive::session::FilletSession;
    rig_fixes();
    let notched = {
        let notch = make_box([0.0, 0.0, 0.9], [0.95, 10.0, 9.1]).unwrap();
        solidcore_ops::boolean::boolean(stair_twin(), notch, solidcore_ops::BooleanOp::Subtract)
            .unwrap()
            .unwrap()
    };
    let mut reasons = Vec::new();
    for b in [stair_twin(), notched] {
        let e1 = nearest_edge(&b, [0.0, -5.0, 0.0]);
        blend(&b, Blend::Round, &[e1], &[1.0], &[], Corners::Filled).unwrap();
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let picks = [
            nearest_edge(&b, [5.0, 0.0, 0.0]),
            nearest_edge(&b, [1.0 - s, 0.0, -1.0 + s]),
        ];
        let (mut k, mut brep) = kernel_copy(&b);
        let chain: Vec<EdgeId> = picks.iter().map(|&i| brep.edge_list()[i]).collect();
        let ends = concave_chain_ends(&k, &mut brep, &chain).unwrap();
        assert_eq!(ends.len(), 1);
        let mut session = FilletSession::new(&mut k, &mut brep).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &chain, 1.0, 1, 1, 1.0)
            .unwrap();
        session.set_roll_over_pass(ends);
        let done = session.do_fillet(&mut k);
        reasons.push((done.is_ok(), session.error_info().no_room_reason()));
    }
    assert_eq!(reasons[0], (true, None), "{reasons:?}");
    assert_eq!(
        reasons[1],
        (false, Some(NoRoomReason::ConcaveOverflow)),
        "{reasons:?}"
    );
}

/// A session's failure says what the session noted of the step that failed
/// ([`solidcore_ops::blend::session_failure`]). The session here is asked to fillet with no
/// edge set up: it makes no fillet face, and what it noted says so after the kernel's statement.
#[test]
fn a_session_failure_says_what_its_session_noted() {
    use solidcore::blend::fillet_executive::session::FilletSession;
    let (mut k, mut brep) = kernel_copy(&block());
    let mut s = FilletSession::new(&mut k, &mut brep).unwrap();
    let e = s.do_filleting(&mut k, None).unwrap_err();
    assert_eq!(
        blend::failure(&e).unwrap().to_string(),
        "the blend failed (status 1001): failed: a step of the fillet executive failed (the \
         error info says which)"
    );
    assert_eq!(
        blend::session_failure(&s, &e).unwrap().to_string(),
        "the blend failed (status 1001): failed: a step of the fillet executive failed (the \
         error info says which): Fillet Surface Computation Failure: no fillet faces were made"
    );
}

/// tangency-labs/solidcraft#12175: a ring (radii 17.5 and 7.5, z -10 to 10) joined by a tab
/// 20 wide (x 10 to 60, z -5 to 5), its edges at the ring and its long edges rounded r 2, a part
/// mirror symmetric about z = 0. A round on the ring's outer circle builds alike at the bottom
/// and at the top. It runs the whole closed edge, so its end section is its start one, but the
/// two are solved apart: at the bottom the turn measured between them came out a rounding short
/// of 360 degrees, a sweep SMLib's check refuses (`SmSurfOfRevolution` Test #7), at r 0.14 and 2
/// here (r 0.13 rounds the other way). Each round takes off the ring's corner, by Pappus
/// 2 pi r^2 ((R - r)(1 - pi/4) + r/6), and its face is the torus R - r, r about the ring's axis.
/// A chamfer round the circle, a cone, builds alike at both ends too.
#[test]
fn a_round_along_a_whole_circle_builds_alike_on_both_ends_of_a_mirrored_ring() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::BooleanOp;

    all_fixes();
    let (outer, inner, half) = (17.5_f64, 7.5, 10.0_f64);
    let circle = |radius: f64, axis: f64, start: f64| Curve::Arc {
        center: [0.0, 0.0, -10.0],
        axis: [0.0, 0.0, axis],
        reference: [1.0, 0.0, 0.0],
        radius,
        start,
        end: start + 2.0 * PI,
    };
    let ring = extrude(
        &[circle(outer, 1.0, 0.0), circle(inner, -1.0, -2.0 * PI)],
        [0.0, 0.0, 1.0],
        20.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let corners = [[10.0, -half], [60.0, -half], [60.0, half], [10.0, half]];
    let outline: Vec<Curve> = (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            Curve::Line {
                start: [a[0], a[1], -5.0],
                end: [b[0], b[1], -5.0],
            }
        })
        .collect();
    let tab = extrude(
        &outline,
        [0.0, 0.0, 1.0],
        10.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let base = boolean(ring, tab, BooleanOp::Union).unwrap().unwrap();
    // the tab's two upright edges and four arcs at the ring, and its four long edges
    let x = (outer * outer - half * half).sqrt();
    let mid = (half / outer).asin() / 2.0;
    let (ax, ay) = (outer * mid.cos(), outer * mid.sin());
    let mut at = vec![[x, -half, 0.0], [x, half, 0.0]];
    for z in [-5.0, 5.0] {
        at.extend([[ax, -ay, z], [ax, ay, z], [35.0, -half, z], [35.0, half, z]]);
    }
    let edges: Vec<usize> = at.iter().map(|&p| edge_at(&base, p)).collect();
    blend(&base, Blend::Round, &edges, &[2.0], &[], Corners::Filled).unwrap();
    assert_eq!(base.counts().unwrap(), [23, 53, 32]);
    let before = base.volume(1e-10).unwrap();
    // each face's kind and a revolution's sweep (degrees), and for a whole turn, how far a
    // 9 x 9 grid of its points lies from the torus of a round r on the circle at z (an ops body
    // reads back through its .smb, which keeps a round's form, not its definition: no
    // procedural_blend() to read here)
    let faces = |b: &Body, r: f64, z: f64| {
        let (k, made) = kernel_copy(b);
        let centre_z = z - r * z.signum();
        let mut off = 0.0_f64;
        let mut kinds: Vec<(&'static str, Option<[f64; 2]>)> = Vec::new();
        for &f in made.face_list() {
            let s = made.face(f).unwrap().surface.as_ref().unwrap();
            let sweep = match s.kind() {
                SurfaceKind::SurfOfRevolution(rev) => {
                    let step = rev.step_uv_domain();
                    let u = step.u_interval();
                    if u.max() - u.min() == 360.0 {
                        for i in 0..9 {
                            for j in 0..9 {
                                let uv = step.evaluate(f64::from(i) / 8.0, f64::from(j) / 8.0);
                                let p = s.evaluate_step_point(&k, uv).unwrap();
                                let rho = p.x.hypot(p.y) - (outer - r);
                                off = off.max((rho.hypot(p.z - centre_z) - r).abs());
                            }
                        }
                    }
                    Some([u.min(), u.max()])
                }
                _ => None,
            };
            kinds.push((surface_kind_name(s), sweep));
        }
        kinds.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (kinds, off)
    };
    let (kinds_before, _) = faces(&base, 2.0, 10.0);
    // the tab's four rounds along the ring's arcs keep their part turns: only a whole closed
    // edge's round turns 360
    let part_turns: Vec<f64> = kinds_before
        .iter()
        .filter_map(|&(_, sweep)| sweep.map(|[lo, hi]| hi - lo))
        .collect();
    assert_eq!(part_turns.len(), 4, "{part_turns:?}");
    assert!(part_turns.iter().all(|&t| t < 90.0), "{part_turns:?}");
    for r in [0.14, 2.0] {
        let corner = 2.0 * PI * r * r * ((outer - r) * (1.0 - PI / 4.0) + r / 6.0);
        for z in [10.0, -10.0] {
            let b = base.copy().unwrap();
            let edge = edge_at(&b, [-outer, 0.0, z]);
            blend(&b, Blend::Round, &[edge], &[r], &[], Corners::Filled)
                .unwrap_or_else(|e| panic!("r {r} at z {z}: {e:?}"));
            let report = b.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "r {r} at z {z}: {}", report.text);
            assert_eq!(b.counts().unwrap(), [24, 55, 33], "r {r} at z {z}");
            let v = b.volume(1e-10).unwrap();
            assert!(
                rel(v, before - corner) < 1e-9,
                "r {r} at z {z}: {v}, want {}",
                before - corner
            );
            let (mut added, off) = faces(&b, r, z);
            for k in &kinds_before {
                let i = added.iter().position(|a| a == k).unwrap();
                added.remove(i);
            }
            assert_eq!(
                added,
                vec![("surface-of-revolution", Some([0.0, 360.0]))],
                "r {r} at z {z}"
            );
            assert!(off < 1e-9, "r {r} at z {z}: {off} off the torus");
        }
    }
    // a chamfer d 2 round the circle (a linear section revolved, stored as its cone) takes off
    // the triangle d^2/2 whose centroid turns at R - d/3: pi d^2 (R - d/3)
    let d = 2.0;
    for z in [10.0, -10.0] {
        let b = base.copy().unwrap();
        let edge = edge_at(&b, [-outer, 0.0, z]);
        blend(&b, Blend::Chamfer, &[edge], &[d], &[], Corners::Filled)
            .unwrap_or_else(|e| panic!("chamfer at z {z}: {e:?}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "chamfer at z {z}: {}", report.text);
        assert_eq!(b.counts().unwrap(), [24, 55, 33], "chamfer at z {z}");
        let v = b.volume(1e-10).unwrap();
        let want = before - PI * d * d * (outer - d / 3.0);
        assert!(rel(v, want) < 1e-9, "chamfer at z {z}: {v}, want {want}");
        let (mut added, _) = faces(&b, d, z);
        for k in &kinds_before {
            let i = added.iter().position(|a| a == k).unwrap();
            added.remove(i);
        }
        assert_eq!(added, vec![("cone", None)], "chamfer at z {z}");
    }
}

/// Acid-1 stage 4: both rims of the upper half torus, including the outer rim's
/// detour around an r0.7 notch, are rounded r0.8 after joining the seam's boss.
#[test]
fn both_flat_face_loops_of_a_notched_half_torus_round() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::sweep::revolve;
    use solidcore_ops::BooleanOp;

    all_fixes();
    let reopen = |body: Body| {
        let (record, _) = body.to_record().unwrap();
        Body::from_record(&record, true).unwrap().0
    };
    let cylinder = |base, radius, height| {
        let (s, c) = 0.6131_f64.sin_cos();
        extrude(
            &[Curve::Arc {
                center: base,
                axis: [0.0, 0.0, 1.0],
                reference: [c, s, 0.0],
                radius,
                start: 0.0,
                end: 2.0 * PI,
            }],
            [0.0, 0.0, 1.0],
            height,
            0.0,
            DraftCorner::Curvature,
            true,
        )
        .unwrap()
    };
    let ring = revolve(
        &[Curve::Arc {
            center: [30.0, 0.0, 0.0],
            axis: [0.0, -1.0, 0.0],
            reference: [1.0, 0.0, 0.0],
            radius: 8.0,
            start: 0.0,
            end: 2.0 * PI,
        }],
        [0.0; 3],
        [0.0, 0.0, 1.0],
        2.0 * PI,
        true,
    )
    .unwrap();
    let below = make_box([-100.0; 3], [200.0, 200.0, 100.0]).unwrap();
    let half = reopen(
        boolean(reopen(ring), below, BooleanOp::Subtract)
            .unwrap()
            .unwrap(),
    );
    let boss = cylinder([29.984954243751982, 0.95001, -1.9], 0.95, 1.9);
    let joined = reopen(boolean(half, boss, BooleanOp::Union).unwrap().unwrap());
    let notch = cylinder([0.0, 38.0, -1.0], 0.7, 4.0);
    let body = reopen(
        boolean(joined, notch, BooleanOp::Subtract)
            .unwrap()
            .unwrap(),
    );
    let edges: Vec<_> = [[-38.0, 0.0, 0.0], [0.0, 37.3, 0.0], [-22.0, 0.0, 0.0]]
        .into_iter()
        .map(|point| nearest_edge(&body, point))
        .collect();
    // solid.fillet propagates the picks across tangent joins by default (2 degrees).
    let edges = tangent_chain(&body, None, &edges, &[], 2.0 * PI / 180.0)
        .unwrap()
        .members;
    let run = || blend(&body, Blend::Round, &edges, &[0.8], &[], Corners::Filled);
    #[cfg(feature = "profile")]
    let (result, profiles) = solidcore_ops::session::profiled(run);
    #[cfg(not(feature = "profile"))]
    let result = run();
    let result = result.unwrap();
    #[cfg(feature = "profile")]
    for call in profiles {
        let certificates: u64 = call
            .profile
            .summary
            .iter()
            .filter(|s| s.name == "blend::certificate::gn_certify_span")
            .map(|s| s.count)
            .sum();
        assert_eq!(
            certificates, 0,
            "circular spines need no generic span certificates"
        );
    }
    assert!(result.dropped.iter().all(|dropped| !dropped));
    assert!(body.is_manifold_solid().unwrap());
    let report = body.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    assert_eq!(body.solid_count().unwrap(), 1);
    // At z=0.1 the rounded rims are at radii 22.4573 and 37.5427;
    // the notch's radius is 1.1127. The boss and notch roof stay intact.
    for (inside, outside) in [
        ([-37.4, 0.0, 0.1], [-37.8, 0.0, 0.1]),
        ([-22.6, 0.0, 0.1], [-22.3, 0.0, 0.1]),
        ([0.0, 36.8, 0.1], [0.0, 37.2, 0.1]),
        ([0.0, 37.35, 3.1], [0.0, 37.35, 2.9]),
    ] {
        assert_eq!(
            body.classify_point(inside).unwrap(),
            solidcore_ops::measure::PointClass::Inside,
            "{inside:?}"
        );
        assert_eq!(
            body.classify_point(outside).unwrap(),
            solidcore_ops::measure::PointClass::Outside,
            "{outside:?}"
        );
    }
    assert_eq!(
        body.classify_point([29.984954243751982, 0.95001, -0.95])
            .unwrap(),
        solidcore_ops::measure::PointClass::Inside
    );
    // Meridian-area and circle-intersection integration, independently derived in
    // plan/findings/acid1-half-torus-flat-rim-round.md.
    let expected = 18892.19338975647;
    let actual = body.volume(1e-10).unwrap();
    assert!(rel(actual, expected) < 1e-8, "{actual} vs {expected}");
}

/// The volume a round of radius `r` (`chamfer: false`) or a chamfer of distance `r` takes off
/// one rim of the upper half of a torus (ring radius `a`, tube radius `b`) cut by the plane
/// through its axis' middle: the outer rim for `side` 1, the inner for -1. The blend is the
/// revolved meridian blend of the tube's circle and the cut line, which meet square. Both are
/// fixed by where the circle's and the line's offsets by `r` meet, the centre (x, r) with
/// |x - a| = sqrt((b - r)^2 - r^2): the round is the circle about it, and the chamfer joins its
/// feet, (x, 0) on the line and the point of the tube's circle along the centre's direction from
/// the tube's centre. By Pappus, the volume is 2 pi times the first moment about the axis of the
/// region between the blend and the corner, integrated over z (the substitution
/// z = h (1 - cos t) / 2 takes the square roots' ends smoothly).
fn half_torus_rim_blend_takes(a: f64, b: f64, r: f64, side: f64, chamfer: bool) -> f64 {
    let x = a + side * ((b - r).powi(2) - r * r).sqrt();
    let foot = [a + (x - a) * b / (b - r), r * b / (b - r)];
    let n = 20_000;
    let mut moment = 0.0;
    for i in 0..n {
        let t = (f64::from(i) + 0.5) * PI / f64::from(n);
        let z = foot[1] * (1.0 - t.cos()) / 2.0;
        let dz = foot[1] * t.sin() / 2.0 * PI / f64::from(n);
        let tube = a + side * (b * b - z * z).sqrt();
        let blend = if chamfer {
            x + (foot[0] - x) * z / foot[1]
        } else {
            x + side * (r * r - (z - r).powi(2)).max(0.0).sqrt()
        };
        moment += (tube * tube - blend * blend).abs() / 2.0 * dz;
    }
    2.0 * PI * moment
}

/// #18270: a round or a chamfer of both rims of a half torus's flat face took minutes on a
/// Ø300 / Ø80 ring and did not finish at Ø3000 / Ø800 or on the Ø1000 / Ø1 ring, the time
/// growing with the edges' length in tolerances; the rims are circles, so the blend is exact
/// (#19190). Each ring is the issue's (blend size 0.05 x the tube's diameter) and takes the
/// closed-form volume.
#[test]
fn both_flat_face_rims_of_a_large_half_torus_blend_to_their_closed_forms() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::sweep::revolve;
    use solidcore_ops::BooleanOp;

    all_fixes();
    for (a, b) in [(150.0, 40.0), (1500.0, 400.0), (500.0, 0.5)] {
        let r = 0.1 * b;
        for kind in [Blend::Round, Blend::Chamfer] {
            let ring = revolve(
                &[Curve::Arc {
                    center: [a, 0.0, 0.0],
                    axis: [0.0, -1.0, 0.0],
                    reference: [1.0, 0.0, 0.0],
                    radius: b,
                    start: 0.0,
                    end: 2.0 * PI,
                }],
                [0.0; 3],
                [0.0, 0.0, 1.0],
                2.0 * PI,
                true,
            )
            .unwrap();
            let reach = a + b + 1.0;
            let below = make_box(
                [-reach, -reach, -b - 1.0],
                [2.0 * reach, 2.0 * reach, b + 1.0],
            )
            .unwrap();
            let body = boolean(ring, below, BooleanOp::Subtract).unwrap().unwrap();
            let picks: Vec<_> = [[0.0, -(a + b), 0.0], [0.0, -(a - b), 0.0]]
                .into_iter()
                .map(|point| nearest_edge(&body, point))
                .collect();
            let edges = tangent_chain(&body, None, &picks, &[], 2.0 * PI / 180.0)
                .unwrap()
                .members;
            let result = blend(&body, kind, &edges, &[r], &[], Corners::Filled).unwrap();
            assert!(result.dropped.iter().all(|dropped| !dropped));
            let report = body.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{}", report.text);
            assert_eq!(body.solid_count().unwrap(), 1);
            let chamfer = kind == Blend::Chamfer;
            let expected = PI * PI * a * b * b
                - half_torus_rim_blend_takes(a, b, r, 1.0, chamfer)
                - half_torus_rim_blend_takes(a, b, r, -1.0, chamfer);
            let actual = volume(&body);
            assert!(
                rel(actual, expected) < 1e-6,
                "{kind:?} a {a} b {b}: {actual} vs {expected}"
            );
        }
    }
}

/// A revolved r8 corner, clipped on x = +/-10. Its torus support must survive the
/// round's definition record and reload, including the partial generator's parameters.
#[test]
fn a_round_on_a_revolved_torus_end_edge_survives_reload() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::sweep::revolve;
    use solidcore_ops::BooleanOp;

    all_fixes();
    let radial = [-3.0_f64.sqrt() / 2.0, 0.5, 0.0];
    let at = |rho: f64, z: f64| [radial[0] * rho, radial[1] * rho, z];
    let profile = [
        Curve::Line {
            start: at(39.4, 8.8),
            end: at(20.0, 8.8),
        },
        Curve::Line {
            start: at(20.0, 8.8),
            end: at(20.0, 4.0),
        },
        Curve::Line {
            start: at(20.0, 4.0),
            end: at(41.0, 4.0),
        },
        Curve::Arc {
            center: at(33.0, 4.0),
            axis: [radial[1], -radial[0], 0.0],
            reference: radial,
            radius: 8.0,
            start: 0.0,
            end: 0.6_f64.asin(),
        },
    ];
    // Independent torus/plane/canal flux integration with div(x+10,0,0)=1.
    // The ball centres are (-10+r, sqrt((33+(8-r)cos(t))^2-(-10+r)^2),
    // 4+(8-r)sin(t)); the canal is trimmed on z=4 and z=8.8. Integrating
    // its circular sections analytically and the remaining t integral numerically
    // gives these values, stable to 1e-10 mm^3 from 8192 to 32768 panels.
    for (radius, expected) in [(1.0, 2009.800477555554), (0.5, 2010.294917895084)] {
        let ring = revolve(&profile, [0.0; 3], [0.0, 0.0, -1.0], 2.0 * PI / 3.0, true).unwrap();
        let clip = make_box([-10.0, 0.0, -10.0], [20.0, 50.0, 30.0]).unwrap();
        let body = boolean(ring, clip, BooleanOp::Intersect).unwrap().unwrap();
        let rho = 33.0 + (64.0_f64 - 2.5 * 2.5).sqrt();
        // This point is on the exact torus/plane intersection. The boolean's edge
        // is its fitted intersection curve, so select it by distance rather than
        // requiring the point to lie on that approximation to 1e-9 mm.
        let point = [-10.0, (rho * rho - 100.0).sqrt(), 6.5];
        let selected = nearest_edge(&body, point);
        blend(
            &body,
            Blend::Round,
            &[selected],
            &[radius],
            &[],
            Corners::Filled,
        )
        .unwrap();
        // Reopen the saved definition, rather than measuring only the live result.
        let (record, _) = body.to_record().unwrap();
        let (reopened, _) = Body::from_record(&record, true).unwrap();
        assert!(reopened.is_manifold_solid().unwrap());
        let report = reopened.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "r{radius}: {}", report.text);
        let actual = volume(&reopened);
        assert!(
            (actual - expected).abs() < 1e-4,
            "r{radius}: {actual} vs {expected}"
        );
    }
}

/// A closed section in the plane x = `x` about (0, 20): 12 cubic Bezier pieces through the
/// points of the circle of radius `r` at every 30 degrees from `phase`. The end tangents
/// are the circle's, a third of the piece's chord long, times `1 + skew` after every third
/// joint from the first and `1 - skew` before it, and the reverse at the joint after: two
/// pieces join with one tangent direction, at speeds `skew` apart.
fn bezier_circle(x: f64, r: f64, phase: f64, skew: f64) -> Vec<Curve> {
    let at = |i: u32| {
        let t = phase + std::f64::consts::TAU * f64::from(i % 12) / 12.0;
        let sign = match i % 3 {
            1 => 1.0,
            2 => -1.0,
            _ => 0.0,
        };
        (
            Vec3::new(x, r * t.sin(), 20.0 - r * t.cos()),
            Vec3::new(0.0, t.cos(), t.sin()),
            skew * sign,
        )
    };
    (0..12)
        .map(|i| {
            let ((p0, t0, k0), (p3, t3, k3)) = (at(i), at(i + 1));
            let third = p0.distance(p3) / 3.0;
            let p1 = p0 + t0 * (third * (1.0 + k0));
            let p2 = p3 - t3 * (third * (1.0 - k3));
            Curve::Nurbs {
                degree: 3,
                points: [p0, p1, p2, p3].map(|p| [p.x, p.y, p.z, 1.0]).to_vec(),
                knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            }
        })
        .collect()
}

/// A round on the end rim of a ruled loft whose one side face is G1 only to about 1e-10
/// rad at its knot lines (tangency-labs/solidcraft#15858). The loft runs from a circle of
/// radius 15 to one of radius 20 turned 0.01 rad, each 12 Bezier pieces that join with one
/// tangent direction. The first circle's pieces join at equal speeds and the second's at
/// speeds 1e-7 apart, and the two sections' tangents at a joint are 0.01 rad apart, so the
/// ruled face's derivative along the section, `(1 - u) a0 T0 + u a1 T1` before a joint and
/// `(1 - u) b0 T0 + u b1 T1` after it, turns the face's normal there. The round's stations
/// on those lines were solved on the pieces after and before them, a jump apart, and the
/// round did not close on itself (`NotSolid`; with the issue's own sections, where the jump
/// is 4e-10 rad, a station's feet did not converge).
///
/// The round must build. What it removes is the cone's round: the section between the end
/// plane and a wall 85.236 degrees from it (the wall leans atan(5 / 60)),
/// `r^2 (cot(alpha / 2) - (pi - alpha) / 2)` = 1.0393 for r = 2, swept round at its
/// centroid's radius, 19.46: 127.1.
#[test]
fn a_round_runs_over_knot_lines_where_a_loft_face_s_normal_turns() {
    all_fixes();
    let sections = [
        bezier_circle(-30.0, 15.0, 0.0, 0.0),
        bezier_circle(30.0, 20.0, 0.01, 1.0e-7),
    ];
    let b = solidcore_ops::sweep::loft(&sections, 1, true).unwrap();
    let before = volume(&b);
    let rim = edge_at(
        &b,
        [30.0, 20.0 * f64::sin(0.01), 20.0 - 20.0 * f64::cos(0.01)],
    );
    let made = blend(&b, Blend::Round, &[rim], &[2.0], &[], Corners::Filled).unwrap();
    assert_eq!(made.dropped, vec![false]);
    let removed = before - volume(&b);
    assert!((removed - 127.1).abs() < 1.0, "removed {removed}");
}

/// The plate of [`shallow_corner_plate`] drafted `draft` degrees in (its walls meet the cap at
/// z = 0, its outline, at 90 - `draft` degrees), with the record indices of the two edges of
/// that cap at the kink.
fn drafted_corner_plate(kink: f64, draft: f64) -> (Body, [usize; 2]) {
    let t = kink.to_radians();
    let (u, n) = ([t.cos(), t.sin()], [t.sin(), -t.cos()]);
    let p = [
        [0.0, 0.0],
        [50.0, 0.0],
        [50.0 + 40.0 * u[0], 40.0 * u[1]],
        [50.0 + 40.0 * u[0] + 30.0 * n[0], 40.0 * u[1] + 30.0 * n[1]],
        [0.0, -30.0],
    ];
    let profile: Vec<Curve> = (0..p.len())
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % p.len()]);
            Curve::Line {
                start: [a[0], a[1], 0.0],
                end: [b[0], b[1], 0.0],
            }
        })
        .collect();
    let b = extrude(
        &profile,
        [0.0, 0.0, 1.0],
        2.0,
        draft.to_radians(),
        DraftCorner::default(),
        true,
    )
    .unwrap();
    let edges = [
        edge_at(&b, [25.0, 0.0, 0.0]),
        edge_at(&b, [50.0 + 20.0 * u[0], 20.0 * u[1], 0.0]),
    ];
    (b, edges)
}

/// #16627: chamfers joined at a near-straight corner leave the shared section `2 o tan(a / 2)`
/// off, `o` their offset from the side faces and `a` the angle between those faces (the corner
/// of each chamfer's offset planes lies on its side face's offset at `o`; the shared one where
/// the two offsets cross), not their setbacks. A chamfer of setbacks `d` on one face and `d'`
/// on the other of a wedge of angle `w` stands `(d' - d cos w) / sin w` off the first: on a 120
/// degree wedge setbacks 1 stand sqrt 3 off. The Join is bounded by the largest offset, so
/// chamfers of setbacks 1, 1 x 0.5 and 0.5 x 1 on wedges of 60 and 120 degrees (walls drafted
/// 30 degrees either way) pass the check at 0.97, 1.03 and 1.5 of the kink where that offset
/// reaches the intersection tolerance 2e-5. Bounded by the setbacks, the setbacks 1 on the 120
/// degree wedge would be joined at 1.5 (`a` 1.73e-5 rad), leaving the vertex sqrt 3 * a =
/// 3.0e-5 off: forced to the Join at 2e-3 degrees, the check finds it 5.2359878e-5 off,
/// sqrt 3 * a to eight digits.
#[test]
fn chamfers_join_at_a_near_straight_corner_within_their_offsets() {
    all_fixes();
    for draft in [30.0_f64, -30.0] {
        // the walls meet the cap at w: cos w = -n_cap . n_wall
        let w = (90.0 - draft).to_radians();
        let (cw, sw) = (w.cos(), w.sin());
        let d = draft.to_radians();
        for (top, side) in [(1.0, 1.0), (1.0, 0.5), (0.5, 1.0)] {
            let offsets = [(side - cw * top) / sw, (top - cw * side) / sw];
            let size = offsets[0].max(offsets[1]);
            let a_max = 2.0 * (2e-5 / (2.0 * size)).atan();
            let k_max = ((a_max.cos() - d.sin().powi(2)) / d.cos().powi(2))
                .acos()
                .to_degrees();
            for f in [0.97, 1.03, 1.5] {
                let kink = f * k_max;
                let (b, edges) = drafted_corner_plate(kink, draft);
                let what = format!("chamfer {top} x {side}, draft {draft}, {kink} degrees");
                if top == side {
                    blend(&b, Blend::Chamfer, &edges, &[top], &[], Corners::Filled)
                        .unwrap_or_else(|e| panic!("{what}: {e:?}"));
                } else {
                    let along: Vec<usize> = edges
                        .iter()
                        .map(|&e| {
                            let faces = edge_faces_along(&b, e, [0.0, 0.0, -1.0]).unwrap();
                            if faces[0].1 > faces[1].1 {
                                faces[0].0
                            } else {
                                faces[1].0
                            }
                        })
                        .collect();
                    chamfer_sides(&b, &edges, &along, top, ChamferSecond::Distance(side))
                        .unwrap_or_else(|e| panic!("{what}: {e:?}"));
                }
                let report = b.check(CheckLevel::All).unwrap();
                assert!(report.is_valid(), "{what}: {report}");
            }
        }
    }
}

/// #13911: the inner top outline of the issue's phone case, as SolidCraft passes it to the
/// chamfer: lines, splines and arcs of radius 1.6 only 0.013 long, 17 edges in one closed loop
/// on the plane z = 9.41 between vertical walls, the case's material outside the loop. At the 8
/// arc-to-spline vertices the walls meet 3.4e-3 degrees off straight. The record is the body
/// SolidCraft's trace gives the blend (`tests/data/issue_13911_inner_top_outline.bodies.json.gz`), and the edges are its picks, in
/// its order (record indices, through the record's order).
const ISSUE_13911_EDGES: [usize; 17] = [
    170, 169, 171, 168, 172, 167, 173, 166, 174, 165, 175, 164, 176, 163, 177, 162, 178,
];

/// The issue's body and its record order.
fn issue_13911_body() -> (Body, Order) {
    let case = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/data/issue_13911_inner_top_outline.bodies.json.gz"
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(case).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b1"].clone()).unwrap();
    Body::from_record(&record, true).unwrap()
}

/// The length of edge `e` of `brep`: `|r'|` integrated by 8-point Gauss-Legendre on 256 spans
/// (exact to rounding for lines and arcs, and far below the tests' tolerances for the cubic
/// splines and the rails). Kept independent of the kernel's own edge length on purpose: it
/// is the test's oracle, a cross-check on the kernel rather than a call into it.
fn edge_length(k: &Kernel, brep: &solidcore::Brep, e: EdgeId) -> f64 {
    const X: [f64; 4] = [
        0.183_434_642_495_649_8,
        0.525_532_409_916_329,
        0.796_666_477_413_626_7,
        0.960_289_856_497_536_3,
    ];
    const W: [f64; 4] = [
        0.362_683_783_378_362,
        0.313_706_645_877_887_3,
        0.222_381_034_453_374_5,
        0.101_228_536_290_376_3,
    ];
    let c = edge::curve(brep, e).unwrap().unwrap().clone();
    let ivl = edge::interval(brep, e).unwrap();
    let (t0, t1) = (ivl.min(), ivl.max());
    let n = 256;
    let h = (t1 - t0) / f64::from(n);
    let mut length = 0.0;
    for j in 0..n {
        let mid = t0 + (f64::from(j) + 0.5) * h;
        for (x, w) in X.iter().zip(W) {
            for t in [mid - x * h / 2.0, mid + x * h / 2.0] {
                let d = c.evaluate(k, t, 1, false, true).unwrap();
                length += w * h / 2.0 * d[1].length();
            }
        }
    }
    length
}

/// The loop's length and the angles its edges meet at in the plane (the kinks), from the
/// input body.
fn issue_13911_loop(b: &Body, edges: &[usize]) -> (f64, Vec<f64>) {
    let (k, brep) = kernel_copy(b);
    let mut length = 0.0;
    // each end: its point and the edge's unit direction leaving it
    let mut ends: Vec<(Vec3, Vec3)> = Vec::new();
    for &i in edges {
        let e = brep.edge_list()[i];
        length += edge_length(&k, &brep, e);
        let c = edge::curve(&brep, e).unwrap().unwrap().clone();
        let ivl = edge::interval(&brep, e).unwrap();
        for (t, sign, left) in [(ivl.min(), 1.0, false), (ivl.max(), -1.0, true)] {
            let d = c.evaluate(&k, t, 1, left, true).unwrap();
            ends.push((d[0], d[1] * (sign / d[1].length())));
        }
    }
    // the kink at each vertex: pi less the angle between the two edges leaving it
    let mut kinks = Vec::new();
    for (i, &(p, t)) in ends.iter().enumerate() {
        for &(q, u) in &ends[i + 1..] {
            if p.distance(q) < 1e-4 {
                kinks.push(t.cross(u).length().atan2(-t.dot(u)));
            }
        }
    }
    (length, kinks)
}

/// The lengths of the blend's two rails round the loop, on the top (z = 9.41) and on the walls
/// (`depth` below it): the result's edges whose middles stand `reach` off the input's loop.
fn issue_13911_rails(
    input: &Body,
    loop_edges: &[usize],
    made: &Body,
    reach: f64,
    depth: f64,
) -> [f64; 2] {
    let (k0, b0) = kernel_copy(input);
    let (k, b) = kernel_copy(made);
    let mut rails = [0.0; 2];
    for &e in b.edge_list() {
        let c = edge::curve(&b, e).unwrap().unwrap().clone();
        let ivl = edge::interval(&b, e).unwrap();
        let mid = c.evaluate(&k, ivl.evaluate(0.5), 0, false, true).unwrap()[0];
        let off = loop_edges
            .iter()
            .map(|&i| {
                edge_closest_point(&k0, &b0, b0.edge_list()[i], mid)
                    .unwrap()
                    .closest
                    .distance
            })
            .fold(f64::INFINITY, f64::min);
        if (off - reach).abs() > 1e-4 {
            continue;
        }
        if (mid.z - 9.41).abs() < 1e-7 {
            rails[0] += edge_length(&k, &b, e);
        } else if (mid.z - (9.41 - depth)).abs() < 1e-7 {
            rails[1] += edge_length(&k, &b, e);
        }
    }
    rails
}

/// #13911: chamfers and rounds of the inner top outline of the issue's phone case build at
/// every size from 0.05 to 1, and each result passes `CheckLevel::All` but for the one failure
/// the input body already has (a cylinder's generator-line scale off by 4.1e-11 against
/// 2e-12, untouched by the blend). From 0.34 they failed with status 1001: the near-straight
/// arc-to-spline corners were mitred (#16627's bound), and two rails crossing 3.4e-3 degrees
/// apart do not meet. There the Join's section lies over the second blend, 1.8e-9 times the
/// size off it (`blend_corner::join_gap`), so the corners join.
///
/// The rails are the loop's offsets, closed forms of a closed planar loop with the material
/// outside it: on the plane the top rail is `L + 2 pi d` long (`L` the loop's length, `2 pi`
/// its total turning, `d` the section's reach along the plane: a chamfer's setback, a round's
/// radius on these upright walls), and on the walls, `d` down, the wall rail is `L` long. The
/// kinks put the top rail off by at most `d` times their angles, and each of the 17 rail pieces
/// may end within twice the B-rep's intersection tolerance of an exact offset's end (that
/// slack is ARGUED, not derived; so is the rails' selection, the result's edges whose middles
/// stand `reach` off the loop at the two heights).
#[test]
fn the_inner_top_outline_of_13911_chamfers_and_rounds_at_every_size() {
    // SolidCraft's fix set: every flag on
    solidcore_ops::session::set_thread_fixes(solidcraft_fixes::solidcraft_fix_names()).unwrap();
    let (input, _) = issue_13911_body();
    let before = input.check(CheckLevel::All).unwrap();
    let known = before.to_string();
    assert!(
        !before.is_valid()
            && known.contains("SmCone::Test #5")
            && known.contains("[owner SmCylinder]"),
        "{known}"
    );
    // the loop, picked natively at the issue's points (the record's edges, independently)
    let picks: Vec<usize> = [
        [34.215, 7.4e-12, 9.41],
        [34.215_026_705_948_8, 56.426_536_712_559_21, 9.41],
        [34.215_026_706_015_58, -56.426_536_712_536_844, 9.41],
        [31.057_880_880_291_407, 69.062_880_880_112_03, 9.41],
        [34.218_953_400_137_07, -57.469_543_122_343_22, 9.41],
        [18.421_536_712_748_598, 72.220_026_705_722_15, 9.41],
        [30.278_812_353_690_66, -69.759_579_427_087_83, 9.41],
        [9.86e-11, 72.219_999_999_896_37, 9.41],
        [18.421_536_712_791_8, -72.220_026_705_723_14, 9.41],
        [-18.421_536_712_551_635, 72.220_026_705_730_56, 9.41],
        [1.41e-10, -72.219_999_999_979_59, 9.41],
        [-31.057_880_880_071_046, 69.062_880_880_150_05, 9.41],
        [-18.421_536_712_469_653, -72.220_026_705_895_69, 9.41],
        [-34.215_026_705_628_176, 56.426_536_712_606_68, 9.41],
        [-31.057_880_880_000_702, -69.062_880_880_201_66, 9.41],
        [-34.214_999_999_792_27, -1.66e-11, 9.41],
        [-34.215_026_705_616_495, -56.426_536_712_632_67, 9.41],
    ]
    .iter()
    .map(|&p| nearest_edge(&input, p))
    .collect();
    let (length, kinks) = issue_13911_loop(&input, &picks);
    assert_eq!(kinks.len(), 17, "{kinks:?}");
    let turned: f64 = kinks.iter().map(|a| a.abs()).sum();
    assert!(turned < 1e-3, "{kinks:?}");
    let xsect = {
        let (k, brep) = kernel_copy(&input);
        solidcore::topology::vertex::corner::xsect_tol_3d_of_brep(&k, &brep)
    };
    assert_eq!(xsect, 2e-5);
    for kind in [Blend::Chamfer, Blend::Round] {
        for size in [0.05, 0.33, 0.34, 0.4, 0.6, 1.0] {
            let what = format!("{kind:?} {size}");
            let (b, order) = issue_13911_body();
            blend_ordered(
                &b,
                &order,
                kind,
                &ISSUE_13911_EDGES,
                &[size],
                &[],
                Corners::Filled,
                BlendOptions::default(),
            )
            .unwrap_or_else(|e| panic!("{what}: {e:?}"));
            let report = b.check(CheckLevel::All).unwrap();
            assert_eq!(report.to_string(), known, "{what}");
            // on the vertical walls under the plane, the section reaches `size` along each face
            let [top, wall] = issue_13911_rails(&input, &picks, &b, size, size);
            let slack = 2.0 * 17.0 * xsect;
            let want_top = length + 2.0 * PI * size;
            assert!(
                (top - want_top).abs() <= size * turned + slack,
                "{what}: top rail {top}, closed form {want_top}"
            );
            assert!(
                (wall - length).abs() <= slack,
                "{what}: wall rail {wall}, closed form {length}"
            );
        }
    }
}

/// One closed section of [`a_rim_round_across_a_loft_s_creases_is_certified_or_refused`]: the
/// cubic Bezier pieces through 12 points at 30 degree steps from `phase` (radians, from the
/// bottom) round an ellipse in the plane x = `x` (half-axes `a` along y and `b` along z,
/// centre (x, 0, 20)), as a fit-point spline
/// gives them, each piece on [0, 1]: the tangent at a point is the chord-length one, so two
/// pieces meet in one direction at speeds in the ratio of their chords. On a circle the chords
/// are equal and the pieces join smoothly; on an ellipse they are not, and a ruled loft to it
/// creases its side along those joints.
fn fit_point_ellipse(x: f64, a: f64, b: f64, phase: f64) -> Vec<Curve> {
    let p: Vec<[f64; 3]> = (0..12)
        .map(|i| {
            let t = phase + f64::from(i) * PI / 6.0;
            [x, a * t.sin(), 20.0 - b * t.cos()]
        })
        .collect();
    let at = |i: usize| p[i % 12];
    let chord = |i: usize| {
        let (q, r) = (at(i), at(i + 1));
        ((r[1] - q[1]).powi(2) + (r[2] - q[2]).powi(2)).sqrt()
    };
    // the tangent at point i over the chord-length parameter
    let tangent = |i: usize| {
        let (q, r) = (at(i + 11), at(i + 1));
        let h = chord(i + 11) + chord(i);
        [(r[1] - q[1]) / h, (r[2] - q[2]) / h]
    };
    (0..12)
        .map(|i| {
            let (q, r, h) = (at(i), at(i + 1), chord(i));
            let (m0, m1) = (tangent(i), tangent(i + 1));
            Curve::Nurbs {
                degree: 3,
                points: vec![
                    [x, q[1], q[2], 1.0],
                    [x, q[1] + h * m0[0] / 3.0, q[2] + h * m0[1] / 3.0, 1.0],
                    [x, r[1] - h * m1[0] / 3.0, r[2] - h * m1[1] / 3.0, 1.0],
                    [x, r[1], r[2], 1.0],
                ],
                knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            }
        })
        .collect()
}

/// #17015: a ruled loft from a circle (radius 15, x = -30) to an ellipse (half-axes 20 and
/// 18, x = 30), both through 12 fit points, its larger rim rounded r 2. The ellipse's pieces
/// meet at unequal speeds, so the loft's side is several faces meeting in creases a few
/// ten thousandths of a radian deep, and the rounds on two of them stand about the radius
/// times that angle apart where they meet on the rim. Joining them set one round's end row to
/// the other's and kept its old bound: the fillet reported success and the body then failed
/// to read back, its blend face's definition not certifying its net (0.016 off, against a
/// bound of 3.9e-6). A round by its definition is now either proved within the tolerance of
/// its definition after the join, its bound the proved one, or the join is refused and the
/// fillet with it: the body reopens from its record, or the fillet fails.
#[test]
fn a_rim_round_across_a_loft_s_creases_is_certified_or_refused() {
    all_fixes();
    let sections = [
        fit_point_ellipse(-30.0, 15.0, 15.0, 0.0),
        fit_point_ellipse(30.0, 20.0, 18.0, 0.0),
    ];
    let b = solidcore_ops::sweep::loft(&sections, 1, true).unwrap();
    assert!(
        b.counts().unwrap()[0] > 3,
        "the loft's side is one face: no crease to round across"
    );
    let rim = edge_at(&b, [30.0, 0.0, 2.0]);
    let chain = tangent_chain(&b, None, &[rim], &[], HALF_DEGREE)
        .unwrap()
        .members;
    assert!(chain.len() > 1, "the rim is one edge: {chain:?}");
    match blend(&b, Blend::Round, &chain, &[2.0], &[], Corners::Filled) {
        Ok(_) => {
            // reopened from its record, as a design is: each blend face certified again
            let (record, _) = b.to_record().unwrap();
            let (reopened, _) = Body::from_record(&record, true)
                .unwrap_or_else(|e| panic!("the rounded loft does not reopen: {e:?}"));
            let report = reopened.check(CheckLevel::All).unwrap();
            assert!(report.is_valid(), "{}", report.text);
        }
        // refused by the kernel: the join's own refusal, or the corner's failure it ends in
        Err(e) => assert!(
            matches!(
                e,
                Error::Unsupported(_)
                    | Error::BlendFailed { .. }
                    | Error::Status(solidcore::error::SM_ERR)
            ),
            "{e:?}"
        ),
    }
}

/// SolidCraft #17263: a 12.2 x 2.2 bar swept along an arc with its orientation kept (the arc from
/// (0, 0, 0) to (30, 0, 0) about (15, 0, 330), dipping to z = 330 - R at its middle, R = hypot(15,
/// 330)), its lower long edge at y = 0 rounded at 0.2. The faces beside the edge are 2.2 and 12.2
/// wide; the round was refused as cutting the part in pieces, naming about 0.07, because the middle
/// of a section's arc, inside the material, was classified as lying outside the part. Past that,
/// the round (a torus about the arc's axis) took its section from the solver's first one, which
/// lies past the edge's end on the lower face's ruled extension, 5.5e-6 off the half-plane through
/// the axis: the revolution failed its own check and the result was refused as no solid. It builds.
/// The bar is 12.2 x 2.2 x 30 (each section x = const is the profile moved). The round takes off
/// its corner section A = (1 - pi/4) r^2, whose centroid lies d = r (10 - 3 pi) / (12 - 3 pi)
/// inside the lower face, swept about the arc's axis through its angle theta (Pappus), and at each
/// end the wedge between the upright end face and the radial plane through the edge's end, A 15 d /
/// 330.
#[test]
fn a_round_on_a_bar_swept_along_a_wide_arc_is_not_refused_as_cutting_the_part() {
    all_fixes();
    let (c, r, w, h) = (330.0_f64, 0.2, 12.2, 2.2);
    let big = 15.0_f64.hypot(c);
    let theta = 2.0 * (15.0 / big).asin();
    let line = |a: [f64; 3], b: [f64; 3]| Curve::Line { start: a, end: b };
    let profile = [
        line([0.0, 0.0, 0.0], [0.0, 0.0, h]),
        line([0.0, 0.0, h], [0.0, w, h]),
        line([0.0, w, h], [0.0, w, 0.0]),
        line([0.0, w, 0.0], [0.0, 0.0, 0.0]),
    ];
    let path = [Curve::Arc {
        center: [15.0, 0.0, c],
        axis: [0.0, -1.0, 0.0],
        reference: [-15.0 / big, 0.0, -c / big],
        radius: big,
        start: 0.0,
        end: theta,
    }];
    let (b, _, _) =
        solidcore_ops::sweep::sweep_placed(&profile, &path, SweepFrame::Translate, true).unwrap();
    assert!(rel(volume(&b), w * h * 30.0) < 1e-9, "{}", volume(&b));
    let e = edge_at(&b, [15.0, 0.0, c - big]);
    // `blend` rounds `b` in place (its result names the faces it made, not a new body)
    let made = blend(&b, Blend::Round, &[e], &[r], &[], Corners::Filled).unwrap();
    assert!(!made.made.is_empty());
    let area = (1.0 - PI / 4.0) * r * r;
    let d = r * (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let want = w * h * 30.0 - area * (big - d) * theta - 2.0 * area * 15.0 * d / c;
    assert!(
        (volume(&b) - want).abs() < 1e-6,
        "{} (want {want})",
        volume(&b)
    );
}

/// #17016's exact pre-fillet body: the public report's six superellipse fit-point sections,
/// lofted and joined to its radius-6 cylinder by the first 20 script commands. The fixture
/// is the final SCBREP05 cache slice saved by SolidCraft 0.44.1 (e151d5ea6866), preserving
/// the cyclic C2 spline sections and the Boolean's intersection curves without refitting.
#[test]
fn a_cylinder_foot_round_rolls_across_the_reported_six_section_loft() {
    // The shipped preset holds these two fixes back; every other fix is enabled.
    solidcore_ops::session::set_thread_fixes(
        solidcore::kernel::UpstreamFixes::NAMES
            .iter()
            .copied()
            .filter(|name| {
                !matches!(
                    *name,
                    "fillet_refuse_rounds_breaking_through" | "fillet_round_ends_on_outline"
                )
            }),
    )
    .unwrap();
    let saved = solidcore_record::native::decode(include_bytes!(
        "data/issue_17016_six_section_loft_boss.brep"
    ))
    .unwrap();
    let (body, _) = Body::from_record(&saved.solid, true).unwrap();
    assert_eq!(body.counts().unwrap()[0], 16);
    let input = body.check(CheckLevel::All).unwrap();
    assert!(input.is_valid(), "the pre-fillet body is invalid: {input}");
    // The report's pick is near the foot, rather than exactly on its curved edge.
    let foot = nearest_edge(&body, [6.0, 0.0, 33.0]);
    let before = volume(&body);
    let made = blend(&body, Blend::Round, &[foot], &[1.0], &[], Corners::Filled)
        .unwrap_or_else(|error| panic!("the reported cylinder foot does not round: {error:?}"));
    assert!(!made.made.is_empty(), "the round made no blend faces");
    assert!(body.counts().unwrap()[0] > 16);
    // A concave foot round fills the space between the boss and the loft.
    assert!(volume(&body) > before, "the foot round added no material");
    let report = body.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "the rounded body is invalid: {report}");
    let (record, _) = body.to_record().unwrap();
    let (reopened, _) = Body::from_record(&record, true)
        .unwrap_or_else(|error| panic!("the rounded body does not reopen: {error:?}"));
    let report = reopened.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "the reopened body is invalid: {report}");
}

/// #17016: the round at the foot of a cylinder boss (radius 6 about the z axis, from z = 25)
/// standing through the top of a hull lofted through six elliptic sections, each the 12 fit
/// pieces of [`fit_point_ellipse`] from 15 degrees, so no piece joint lies on the top. The
/// boss meets the hull's top face in one closed edge; the round's rails cross the joints
/// beside it, so the rollover splits the round into pieces, the first starting at the edge's
/// seam and the last ending there. The seam is both ends of the closed edge's parameter, and
/// the last piece's end belongs at the parameter's end: taken at its start, the piece was
/// traced the long way round the edge and its rail trimmed to a negative interval, failing
/// the fillet ("Unable to Process Rollover").
#[test]
fn a_round_split_by_rollover_ends_its_last_piece_at_a_closed_edge_s_seam() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::BooleanOp;
    all_fixes();
    let phase = PI / 12.0;
    let sections = [
        fit_point_ellipse(-50.0, 8.0, 5.0, phase),
        fit_point_ellipse(-30.0, 18.0, 10.0, phase),
        fit_point_ellipse(-10.0, 24.0, 14.0, phase),
        fit_point_ellipse(10.0, 24.5, 14.5, phase),
        fit_point_ellipse(30.0, 20.0, 11.0, phase),
        fit_point_ellipse(50.0, 8.5, 5.5, phase),
    ];
    let hull = solidcore_ops::sweep::loft(&sections, 3, true).unwrap();
    let boss = make_cylinder([0.0, 0.0, 25.0], 6.0, 14.0).unwrap();
    let b = boolean(hull, boss, BooleanOp::Union).unwrap().unwrap();
    // the boss's foot: the edge nearest its side at x = 6 on the hull's top
    let (k, brep) = kernel_copy(&b);
    let foot = Vec3::new(6.0, 0.0, 34.0);
    let distance = |e: EdgeId| edge_closest_point(&k, &brep, e, foot).unwrap().closest;
    let (index, near) = brep
        .edge_list()
        .iter()
        .enumerate()
        .map(|(i, &e)| (i, distance(e)))
        .min_by(|a, c| a.1.distance.total_cmp(&c.1.distance))
        .unwrap();
    assert!(
        (near.point.x.hypot(near.point.y) - 6.0).abs() < 1e-6 && near.point.z > 25.0,
        "the edge picked is not the boss's foot: {near:?}"
    );
    drop(k);
    let r = 1.0;
    let before = volume(&b);
    let [faces, _, _] = b.counts().unwrap();
    let made = blend(&b, Blend::Round, &[index], &[r], &[], Corners::Filled)
        .unwrap_or_else(|e| panic!("the boss's foot does not round: {e:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    // The round adds its faces, one a piece, and cuts one face in two: the hull's top face
    // between the spline joints on either side of the foot (at |y| < 6.45 there, under the
    // round's reach of 6 + r) is crossed by the round from joint to joint on both sides of
    // the boss, leaving a front and a back face. Every other face it meets is trimmed back.
    // Its edge and vertex counts depend on how many pieces the rollover makes along the
    // joints, which the construction does not fix.
    assert!(!made.made.is_empty());
    assert_eq!(b.counts().unwrap()[0], faces + made.made.len() + 1);
    // The round fills the concave foot. Its section in a plane through the boss's axis is the
    // region between the two walls and the ball's arc: r^2 (cot(a / 2) - (pi - a) / 2) for
    // walls meeting at a (through the air). The boss is upright, and the hull's top under the
    // foot slopes less than atan(0.16) (steepest across the sections, half-axes 24 and 14 at
    // |y| = 6: dz/dy = (14 / 24^2) 6 / sqrt(1 - (6 / 24)^2) = 0.151; along x the half-axes
    // change by 0.5 over 20), so a lies in [88, 100] degrees and the section's area in
    // [0.141, 0.233] r^2. By Pappus the added volume is the integral of that area round the
    // axis at its centroid's radius, between 6 and 6 + r: in [0.141 2 pi 6, 0.233 2 pi 7] r^3
    // = [5.3, 10.3] r^3 for r = 1.
    let added = volume(&b) - before;
    assert!((5.3..=10.3).contains(&added), "{added}");
    // Each face the round made lies within the ball's reach of the foot: no farther out from
    // the boss's side than r cot(44 degrees) < 1.04 r, and above the hull's top under the
    // foot (z = 20 + 14 cos(asin(6 / 24)) = 33.55 at |y| = 6 on the lower of the two
    // sections beside it) less r, with 0.5 for the loft between them, up to the boss's top.
    let (k, brep) = kernel_copy(&b);
    for &f in &made.made {
        let face = brep.face_list()[f];
        let bx = face_bounding_box(&k, &brep, face, true).unwrap();
        for p in [bx.min(), bx.max()] {
            assert!(
                p.x.abs() <= 6.0 + 1.04 * r && p.y.abs() <= 6.0 + 1.04 * r,
                "{bx:?}"
            );
            assert!(p.z >= 33.55 - r - 0.5 && p.z <= 39.0, "{bx:?}");
        }
    }
}

/// A 60 x 90 block 15.8 high with its upright corners rounded R 7, the top outline chamfered 5
/// (45 degrees) all round, then the edge where the chamfer meets the walls rounded R 5, as one
/// tangent chain. Each rail end of the round sits on a seam between a flat wall and a corner's
/// cylinder (or between their chamfers' plane and cone), a quarter turn of a cylinder whose
/// offset continues as itself. The rail-end solve took the cylinder's whole closed turn, wider
/// than the extension it searches on upstream, and left the extension for the circle's far
/// side: the round was refused ("Fillet Corner Error: Corner Edge Computation Failure"). Taken
/// from SolidCraft, where a tub's chamfered outline rounded so on 2026-11-09 and was refused
/// after #13538.
///
/// Closed form: every face is one profile in the (inset `d` from the wall, `z`) plane swept
/// round the outline, whose section at inset `d` is the 60 x 90 rectangle with corners R
/// `7 - d`, area `A(d) = (60 - 2d)(90 - 2d) - (4 - pi)(7 - d)^2`. The profile is the wall
/// `d = 0` up to the round's centre height `zc = T - 5 sqrt 2`, the R 5 arc
/// `d = 5 - 5 cos t`, `z = zc + 5 sin t` for `t` in `[0, pi/4]`, and the chamfer
/// `d = z - (T - 5)` from `d = 5 - 5/sqrt 2` up to the top.
#[test]
fn a_round_along_a_chamfered_rounded_outline_builds() {
    const W: f64 = 60.0;
    const H: f64 = 90.0;
    const R: f64 = 7.0;
    const T: f64 = 15.8;
    all_fixes();
    let line = |a: [f64; 2], b: [f64; 2]| Curve::Line {
        start: [a[0], a[1], 0.0],
        end: [b[0], b[1], 0.0],
    };
    let arc = |c: [f64; 2], start: f64| Curve::Arc {
        center: [c[0], c[1], 0.0],
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: R,
        start,
        end: start + PI / 2.0,
    };
    let outline = [
        line([R, 0.0], [W - R, 0.0]),
        arc([W - R, R], -PI / 2.0),
        line([W, R], [W, H - R]),
        arc([W - R, H - R], 0.0),
        line([W - R, H], [R, H]),
        arc([R, H - R], PI / 2.0),
        line([0.0, H - R], [0.0, R]),
        arc([R, R], PI),
    ];
    let b = extrude(
        &outline,
        [0.0, 0.0, 1.0],
        T,
        0.0,
        DraftCorner::default(),
        true,
    )
    .unwrap();
    let chain = |b: &Body, z: f64| {
        let pick = edge_at(b, [W / 2.0, 0.0, z]);
        let chain = tangent_chain(b, None, &[pick], &[], HALF_DEGREE).unwrap();
        assert!(chain.undecided.is_empty(), "{chain:?}");
        chain.members
    };
    let top = chain(&b, T);
    assert_eq!(top.len(), 8, "{top:?}");
    blend(&b, Blend::Chamfer, &top, &[5.0; 8], &[], Corners::Filled).unwrap();
    let foot = chain(&b, T - 5.0);
    assert_eq!(foot.len(), 8, "{foot:?}");
    blend(&b, Blend::Round, &foot, &[5.0; 8], &[], Corners::Filled)
        .unwrap_or_else(|e| panic!("the chamfer's foot does not round: {e:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    let (a0, a1, a2) = (
        W * H - (4.0 - PI) * R * R,
        -2.0 * (W + H) + 2.0 * (4.0 - PI) * R,
        PI,
    );
    let antiderivative = |d: f64| a0 * d + a1 * d * d / 2.0 + a2 * d * d * d / 3.0;
    let s = std::f64::consts::SQRT_2;
    let zc = T - 5.0 * s;
    // the arc: int A(5 - 5 cos t) 5 cos t dt over [0, pi/4], from int cos = sqrt 2 / 2,
    // int cos^2 = pi/8 + 1/4 and int cos^3 = 5 sqrt 2 / 12
    let arc = 5.0 * (a0 + 5.0 * a1 + 25.0 * a2) * s / 2.0
        - 25.0 * (a1 + 10.0 * a2) * (PI / 8.0 + 0.25)
        + 125.0 * a2 * 5.0 * s / 12.0;
    let expected = a0 * zc + arc + antiderivative(5.0) - antiderivative(5.0 - 5.0 / s);
    assert!(
        rel(volume(&b), expected) < 1e-9,
        "{} vs the closed form {expected}",
        volume(&b)
    );
}

/// The ring and the wedge of tangency-labs/solidcraft#17715: a ring about x, its outer side a
/// band of the torus of radii 40 and 5 (the generating circle's arc from 175 to 210 degrees in
/// the meridian half-plane, x = 5 cos t, rho = 40 + 5 sin t), and a wedge revolved 16 degrees
/// about x, between the half-planes at 12 and 28 degrees from the ring's seam, the half-plane
/// at angle theta running along (0, sin theta, -cos theta) (the issue's design, sketched on XZ).
/// The wedge's profile is the arc from 180 to 210 degrees, the cone line to (x, rho) =
/// (-1, 34), the cylinder line to (-5, 34) and the plane line x = -5 up to the arc's leftmost
/// point, where it is tangent to the arc.
fn ring_and_wedge() -> Body {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::sweep::revolve;
    use solidcore_ops::BooleanOp;

    let meridian = |theta: f64| {
        let e = [0.0, theta.sin(), -theta.cos()];
        let at = move |x: f64, rho: f64| [x, rho * e[1], rho * e[2]];
        let arc = move |from: f64, to: f64| Curve::Arc {
            center: at(0.0, 40.0),
            // x cross e, so that the arc's angle t runs from x towards e
            axis: [0.0, -e[2], e[1]],
            reference: [1.0, 0.0, 0.0],
            radius: 5.0,
            start: from.to_radians(),
            end: to.to_radians(),
        };
        (at, arc)
    };
    let line = |start: [f64; 3], end: [f64; 3]| Curve::Line { start, end };
    let (at, arc) = meridian(0.0);
    let t = |deg: f64| {
        let r = f64::to_radians(deg);
        (5.0 * r.cos(), 40.0 + 5.0 * r.sin())
    };
    let (x0, r0) = t(175.0);
    let (x1, r1) = t(210.0);
    let ring_profile = [
        arc(175.0, 210.0),
        line(at(x1, r1), at(0.0, 32.0)),
        line(at(0.0, 32.0), at(0.0, 48.0)),
        line(at(0.0, 48.0), at(x0, r0)),
    ];
    let ring = revolve(&ring_profile, [0.0; 3], [1.0, 0.0, 0.0], 2.0 * PI, true).unwrap();
    let (at, arc) = meridian(12.0_f64.to_radians());
    let wedge_profile = [
        arc(180.0, 210.0),
        line(at(x1, r1), at(-1.0, 34.0)),
        line(at(-1.0, 34.0), at(-5.0, 34.0)),
        line(at(-5.0, 34.0), at(-5.0, 40.0)),
    ];
    let wedge = revolve(
        &wedge_profile,
        [0.0; 3],
        [1.0, 0.0, 0.0],
        16.0_f64.to_radians(),
        true,
    )
    .unwrap();
    boolean(ring, wedge, BooleanOp::Union).unwrap().unwrap()
}

/// A round on the short edge where the wedge's end cap meets the ring's torus band, the arc
/// from 180 to 210 degrees of the generating circle in the cap's half-plane at 28 degrees
/// (tangency-labs/solidcraft#17715), builds. The cap lies between that arc and the plane line
/// x = -5, which leaves the arc's 180 degree end along it: the cap closes to a cusp there, and
/// the round's rail on it, the circle of radius 5 + r about the arc's centre, lies past x = -5
/// wherever |t - 180| < acos(5 / (5 + r)), 19.4 degrees for r 0.3. There the material the
/// round adds is trimmed flush with the plane x = -5 continued, and the band's rail ends on
/// the band's tangent seam with that plane, continued past the cap's plane (without it the
/// corner fails with status 1001). At the arc's 210 degree end the round ends on the plane
/// square to the arc: the ring's rim circle there is square to the arc, and leans over the
/// round only by its tangent's rounding (taken for a lean, the cap was tilted, and the faces
/// the rails enclose were left open past them).
///
/// The added volume is the region within r of both the cap's plane and the band, outside the
/// balls about the round's spine (centres 5 + r from the generating circle's centre in the
/// half-plane at 28 + asin(r / rho) degrees), with x >= -5 and on the round's side of the end
/// plane square to the arc at its 210 degree end. Integrated by Monte Carlo (10^7 points,
/// about 0.3 % of error) it is 0.03675 without the sliver between the band and the plane
/// x = -5 past the tangent circle, and 0.03742 with it; the bounds take 2 % more each way.
#[test]
fn a_round_on_a_revolved_end_cap_that_closes_to_a_cusp_builds() {
    all_fixes();
    let b = ring_and_wedge();
    let before = volume(&b);
    let theta = 28.0_f64.to_radians();
    let on_cap = |x: f64, rho: f64| Vec3::new(x, rho * theta.sin(), -rho * theta.cos());
    // the arc's chord's middle picks the arc, whose ends are the generating circle's points at
    // 210 and 180 degrees
    let e = nearest_edge(
        &b,
        [
            -4.665063509461097,
            38.75 * theta.sin(),
            -38.75 * theta.cos(),
        ],
    );
    let (k, brep) = kernel_copy(&b);
    let edge_id = brep.edge_list()[e];
    let curve = edge::curve(&brep, edge_id).unwrap().unwrap();
    let ivl = edge::interval(&brep, edge_id).unwrap();
    let ends = [ivl.min(), ivl.max()].map(|t| curve.evaluate_point(&k, t).unwrap());
    for want in [on_cap(-2.5 * 3.0_f64.sqrt(), 37.5), on_cap(-5.0, 40.0)] {
        assert!(ends.iter().any(|p| p.distance(want) < 1e-6), "{ends:?}");
    }
    let made = blend(&b, Blend::Round, &[e], &[0.3], &[], Corners::Filled)
        .unwrap_or_else(|e| panic!("the round on the cap's arc does not build: {e:?}"));
    assert!(!made.made.is_empty());
    assert!(b.is_manifold_solid().unwrap());
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    let added = volume(&b) - before;
    assert!((0.0360..=0.0381).contains(&added), "{added}");
}

/// A round of radius `r` in the corner `c` of a meridian section (x, rho), between the
/// rays from `c` along the unit directions `d1` and `d2` (the side of the angle less than pi
/// between them): its area and the rho of its centroid. The region is the kite of `c`, the
/// two feet at `r cot(phi / 2)` along the rays and the ball's centre, less the ball's sector
/// between the feet, of angle `pi - phi`.
fn corner_round(c: [f64; 2], d1: [f64; 2], d2: [f64; 2], r: f64) -> (f64, f64) {
    let phi = (d1[0] * d2[0] + d1[1] * d2[1]).acos();
    let t = r / (phi / 2.0).tan();
    let bisector = [d1[0] + d2[0], d1[1] + d2[1]];
    let length = bisector[0].hypot(bisector[1]);
    let b = [bisector[0] / length, bisector[1] / length];
    let reach = r / (phi / 2.0).sin();
    let centre = c[1] + b[1] * reach;
    let foot = |d: [f64; 2]| c[1] + d[1] * t;
    // each of the kite's two triangles c, foot, centre has area r t / 2
    let kite = r * t;
    let kite_rho = (2.0 * c[1] + foot(d1) + foot(d2) + 2.0 * centre) / 6.0;
    let half = (PI - phi) / 2.0;
    let sector = r * r * half;
    // a sector's centroid lies 2 r sin(half) / (3 half) from its centre, towards c
    let sector_rho = centre - b[1] * 2.0 * r * half.sin() / (3.0 * half);
    let area = kite - sector;
    (area, (kite * kite_rho - sector * sector_rho) / area)
}

/// #19724: a frustum about x (radius 24 at x = 0, 36 at x = 48) with a 12 degree rib on it,
/// whose side leaves the cone at (18, 28.5) 15 degrees off the cone's straight side, the edge
/// there rounded 1 mm: the body SolidCraft wrote after that round
/// (`tests/data/issue_19724_cone_rib_round.bodies.json.gz`), read here with that round as its
/// net. The rib's two sides are cones of one 12 degree arc span each, whose NURBS forms carry
/// the rounding of the conversions that made them: their own numbers prove them, the cones
/// read off their control rows do not (`AnalyticSupport::build`'s parametrisation test).
///
/// Rounding the rib's top edge makes an exact round of the `Circle` class between the two
/// sides, its supports the sides' surfaces. On main its record labelled them cones, and
/// reading it back refused the body ("a support's analytic surface is not its NURBS form's
/// parametrisation"), as SolidCraft's measuring did after the issue's round. A support is
/// now labelled only where reading derives it back from its form, and the body reads back
/// with its round.
///
/// The volume before is the frustum's, the rib's (its triangle turned 12 degrees about x, its
/// base on the cone's side) and the 15 degree round's (the corner between the cone's side and
/// the rib's, filled to the ball, turned 12 degrees), each by Pappus; the top round takes off
/// the rib's 75 degree corner at its top, turned 12 degrees. It ends on the rib's two end
/// planes as the 15 degree round does: one face more, three edges (the top edge split in two,
/// an arc on each end plane) and two vertices.
#[test]
fn a_round_on_a_short_rib_s_cones_reads_back_from_its_record() {
    use solidcore::topology::face::p2 as face;
    use solidcore_record::brep::Surface as RecordSurface;
    rig_fixes();
    let case = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/data/issue_19724_cone_rib_round.bodies.json.gz"
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(case).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut record: Record = serde_json::from_value(records["b1"].clone()).unwrap();
    // the 15 degree round as its net (its definition's supports were written the way this
    // test shows refused)
    let mut nets = 0;
    for f in &mut record.faces {
        if matches!(f.surface, RecordSurface::Blend { .. }) {
            f.surface = f.nurbs.take().unwrap();
            nets += 1;
        }
    }
    assert_eq!(nets, 1);
    let counts = [
        record.faces.len(),
        record.edges.len(),
        record.vertices.len(),
    ];
    assert_eq!(counts, [8, 15, 10]);
    let b = Body::from_record(&record, true).unwrap().0;
    assert_eq!(b.counts().unwrap(), counts);
    // exact rounds on the body itself: a copy through its `.smb` bytes keeps no definitions
    let exact_rounds = |b: &Body| {
        let brep = b.kernel();
        brep.face_list()
            .iter()
            .filter(|&&f| {
                let s = face::surface(&brep, f).unwrap().unwrap();
                matches!(s.procedural_blend(), Some(BlendRef::Exact { .. }))
            })
            .count()
    };
    assert_eq!(exact_rounds(&b), 0);

    let theta = 12.0_f64.to_radians();
    let frustum = PI * 48.0 / 3.0 * (24.0 * 24.0 + 24.0 * 36.0 + 36.0 * 36.0);
    let p: [[f64; 2]; 3] = [
        [18.0, 28.5],
        [26.743_129, 33.353_628],
        [27.370_857, 30.842_714],
    ];
    let cross =
        (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
    let rib = cross.abs() / 2.0 * (p[0][1] + p[1][1] + p[2][1]) / 3.0 * theta;
    let unit = |d: [f64; 2]| {
        let l = d[0].hypot(d[1]);
        [d[0] / l, d[1] / l]
    };
    let towards = |from: [f64; 2], to: [f64; 2]| unit([to[0] - from[0], to[1] - from[1]]);
    let along_cone = unit([1.0, 0.25]);
    let up_rib = towards(p[0], p[1]);
    let turn = (along_cone[0] * up_rib[0] + along_cone[1] * up_rib[1]).acos();
    assert!(
        (turn.to_degrees() - 15.0).abs() < 1e-5,
        "{}",
        turn.to_degrees()
    );
    // the 15 degree round fills the corner outside both sides: between the cone's side back
    // towards x = 0 and the rib's side
    let (area, rho) = corner_round(p[0], [-along_cone[0], -along_cone[1]], up_rib, 1.0);
    let want = frustum + rib + area * rho * theta;
    // the volume's accuracy, and the rib's base 2.5e-7 inside the cone at its far end
    let accuracy = 1e-10;
    let tol = 2.0 * accuracy * want + 1e-5;
    let before = b.volume(accuracy).unwrap();
    assert!((before - want).abs() <= tol, "{before} {want}");

    // the rib's top edge, the arc through its top corner
    let top = nearest_edge(&b, [p[1][0], p[1][1], 0.0]);
    let r = 0.5;
    blend(&b, Blend::Round, &[top], &[r], &[], Corners::Filled)
        .unwrap_or_else(|e| panic!("the rib's top does not round: {e:?}"));
    assert_eq!(b.counts().unwrap(), [9, 18, 12]);
    assert_eq!(exact_rounds(&b), 1);
    let (area, rho) = corner_round(p[1], towards(p[1], p[0]), towards(p[1], p[2]), r);
    let want = want - area * rho * theta;

    let (written, _) = b.to_record().unwrap();
    let back = Body::from_record(&written, true)
        .unwrap_or_else(|e| panic!("the body does not read back: {e:?}"))
        .0;
    assert_eq!(back.counts().unwrap(), [9, 18, 12]);
    assert!(back.is_manifold_solid().unwrap());
    let report = back.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    assert_eq!(exact_rounds(&back), 1);
    let after = back.volume(accuracy).unwrap();
    assert!((after - want).abs() <= tol, "{after} {want}");
}

/// #17616: the issue's sheet-metal part, 3 thick with 2 mm bends: a 90 x 75 base, a three-bend
/// flange on its back edge, and on its right edge a flange split into an 8 mm and a 50 mm
/// segment. The record is the body SolidCraft's trace gives the round
/// (`tests/data/issue_17616_split_flange_tab.bodies.json.gz`).
fn issue_17616_body() -> Body {
    let case = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../solidcore/tests/data/issue_17616_split_flange_tab.bodies.json.gz"
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(case).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b1"].clone()).unwrap();
    Body::from_record(&record, true).unwrap().0
}

/// A cylinder face: its radius, its axis (a point and the unit direction), its UV domain and
/// its surface's natural domain (u and v, each from and to).
type Cylinder = (f64, [Vec3; 2], [f64; 4], [f64; 4]);

/// Each cylinder face of `b`.
fn issue_17616_cylinders(b: &Body) -> Vec<Cylinder> {
    use solidcore::topology::face::p2 as face;
    let (k, brep) = kernel_copy(b);
    let mut out = Vec::new();
    for &f in brep.face_list() {
        let s = face::surface(&brep, f).unwrap().unwrap();
        let SurfaceKind::Cone(c) = s.kind() else {
            continue;
        };
        let cone = c.canonical();
        assert!(cone.semi_angle_deg == 0.0, "a face on a cone");
        let d = face::uv_domain(&brep, f).unwrap();
        let n = s.natural_uv_domain_with_nurb(&k).unwrap();
        out.push((
            cone.radius,
            [cone.origin.origin(), cone.origin.z_axis()],
            [d.u_min(), d.u_max(), d.v_min(), d.v_max()],
            [n.u_min(), n.u_max(), n.v_min(), n.v_max()],
        ));
    }
    out
}

/// #17616: an R10 round on the 50 mm tab's top outside edge (x = 90, z = 50, y 35 to 70)
/// overflows the 3 mm top face and runs onto the inside face x = 87. Its ball's centre is
/// (80, 40) in x, z, so the round takes off, per unit length, the tab's 3 x 10 corner less the
/// disc strip under the arc: `30 - I(7, 10; 10)`, over the 35 of the tab. The top face goes and
/// the round comes in its place, bounded by its rail on x = 90, its crease on x = 87 and its
/// arcs on the tab's ends: 45 faces, 93 edges, 50 vertices, as before.
///
/// On main the round's last step (sewn faces on one surface made one face) tested the edge
/// between the two bends of the split flange, which lie on one cylinder, for removal although
/// it is kept, and the test expanded one bend's surface onto both, its parameterisation
/// changing, while the bend kept its UV domain. Its trims dropped off that domain, and the
/// round was not closed (ops) or its trim was fitted on a third of its edge (SolidCraft's
/// export: "a UV trim on [0, 0.33], its edge on [0, 1]"). Every bend now keeps its surface.
#[test]
fn a_round_overflowing_a_split_flange_s_tab_top_leaves_the_bends_on_their_surfaces() {
    rig_fixes();
    let b = issue_17616_body();
    let before = volume(&b);
    // the ten bends, of radius 2 inside and 5 outside, each with its UV domain its surface's
    // whole natural domain
    let bends_on_their_surfaces = |b: &Body| {
        let bends: Vec<_> = issue_17616_cylinders(b)
            .into_iter()
            .filter(|c| c.0 < 6.0)
            .collect();
        assert_eq!(bends.len(), 10);
        for (r, _, d, n) in &bends {
            assert!(
                (r - 2.0).abs() <= 1e-9 || (r - 5.0).abs() <= 1e-9,
                "a bend of radius {r}"
            );
            for (x, y) in d.iter().zip(n) {
                assert!(
                    (x - y).abs() <= 1e-9,
                    "a bend's domain {d:?} is not its surface's {n:?}"
                );
            }
        }
    };
    // each bend's surface by its radius, its axis line (the foot of the perpendicular from the
    // origin and the direction up to sign) and its natural domain's two ranges (in either
    // order: the round's stitch may swap a face's u and v), sorted
    let bend_surfaces = |b: &Body| {
        let mut keys: Vec<[f64; 11]> = issue_17616_cylinders(b)
            .into_iter()
            .filter(|c| c.0 < 6.0)
            .map(|(r, [o, d], _, n)| {
                let foot = o - d * o.dot(d);
                let d = if d.x + d.y + d.z < 0.0 { d * -1.0 } else { d };
                let (mut s, mut t) = ([n[0], n[1]], [n[2], n[3]]);
                if s[0] > t[0] {
                    std::mem::swap(&mut s, &mut t);
                }
                [
                    r, foot.x, foot.y, foot.z, d.x, d.y, d.z, s[0], s[1], t[0], t[1],
                ]
            })
            .collect();
        keys.sort_by(|a, c| {
            a.iter()
                .zip(c)
                .map(|(x, y)| x.total_cmp(y))
                .fold(std::cmp::Ordering::Equal, std::cmp::Ordering::then)
        });
        keys
    };
    bends_on_their_surfaces(&b);
    let surfaces = bend_surfaces(&b);
    assert_eq!(issue_17616_cylinders(&b).len(), 10);
    let e = edge_at(&b, [90.0, 52.5, 50.0]);
    blend(&b, Blend::Round, &[e], &[10.0], &[], Corners::Filled)
        .unwrap_or_else(|e| panic!("the tab's top edge does not round: {e:?}"));
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    assert_eq!(b.counts().unwrap(), [45, 93, 50]);
    let removed = 35.0 * (30.0 - disc_strip(7.0, 10.0, 10.0));
    let after = volume(&b);
    assert!(
        rel(before - after, removed) < 1e-6,
        "{before} - {after} vs {removed}"
    );
    // the round: one exact cylinder of radius 10 about the ball centres' line x = 80, z = 40
    let rounds: Vec<_> = issue_17616_cylinders(&b)
        .into_iter()
        .filter(|c| c.0 >= 6.0)
        .collect();
    assert_eq!(rounds.len(), 1);
    let (r, [origin, axis], _, _) = rounds[0];
    assert!((r - 10.0).abs() <= 1e-9, "the round's radius {r}");
    assert!(
        axis.cross(Vec3::new(0.0, 1.0, 0.0)).length() <= 1e-12,
        "the round's axis {axis:?}"
    );
    assert!(
        (origin.x - 80.0).abs() <= 1e-9 && (origin.z - 40.0).abs() <= 1e-9,
        "{origin:?}"
    );
    // the bends the round does not reach as they were: each on its own surface, unexpanded
    bends_on_their_surfaces(&b);
    for (was, now) in surfaces.iter().zip(&bend_surfaces(&b)) {
        for (x, y) in was.iter().zip(now) {
            assert!(
                (x - y).abs() <= 1e-9,
                "a bend's surface {was:?} is now {now:?}"
            );
        }
    }
}

/// The volume a round of radius `f` adds where a sphere of radius `r` stands with its centre
/// on a plane (or takes off where a socket of radius `r` is cut so): the ring between the
/// plane, the sphere and the ball of radius `f` touching both, by Pappus. In a half-plane
/// through the axis the ball's centre is at height `f` and distance `rho` from the axis with
/// `r + f` from the sphere's centre; at height z the ring spans from the sphere,
/// `sqrt(r^2 - z^2)`, to the ball, `rho - sqrt(f^2 - (z - f)^2)`, up to the sphere's contact
/// at `zt = f r / (r + f)`.
fn sphere_on_plane_round(r: f64, f: f64) -> f64 {
    let rho = ((r + f).powi(2) - f * f).sqrt();
    let zt = f * r / (r + f);
    // the integral of sqrt(f^2 - t^2)
    let circle = |t: f64| t / 2.0 * (f * f - t * t).max(0.0).sqrt() + f * f / 2.0 * (t / f).asin();
    PI * (2.0 * r * f * zt + f * zt * zt - 2.0 * rho * (circle(zt - f) - circle(-f)))
}

/// A turn of `degrees` about axis `axis` (0, 1, 2 for x, y, z), as `Body::transform` takes it.
fn turn_about(axis: usize, degrees: f64) -> [[f64; 4]; 4] {
    let (s, c) = degrees.to_radians().sin_cos();
    let (i, j) = [(1, 2), (2, 0), (0, 1)][axis];
    let mut m = [[0.0; 4]; 4];
    for (d, row) in m.iter_mut().enumerate() {
        row[d] = 1.0;
    }
    m[i][i] = c;
    m[j][j] = c;
    m[i][j] = s;
    m[j][i] = -s;
    m
}

/// The relative accuracy the rim-round tests ask of the volume integrator.
const RIM_VOLUME_ACCURACY: f64 = 1e-12;

/// A plate `r` thick with its face on the plane through the origin square to `axis`, lying on
/// the side of it `side` gives, and a sphere of radius `r` centred on the origin, first moved
/// by `moves` (applied in order), then joined (a nub) or cut (a socket) by `op`. The plate is
/// 5 r across, wide enough for the round's ring, so the volume measured stays near the ring's.
fn sphere_on_plate(
    r: f64,
    axis: usize,
    side: f64,
    moves: &[[[f64; 4]; 4]],
    op: solidcore_ops::BooleanOp,
) -> Body {
    let mut lo = [-2.5 * r; 3];
    let mut size = [5.0 * r; 3];
    lo[axis] = if side > 0.0 { 0.0 } else { -r };
    size[axis] = r;
    let plate = make_box(lo, size).unwrap();
    let sphere = solidcore_ops::make_sphere([0.0; 3], r).unwrap();
    for m in moves {
        sphere.transform(m).unwrap();
    }
    solidcore_ops::boolean::boolean(plate, sphere, op)
        .unwrap()
        .unwrap()
}

/// The edges of `b` on the circle of radius `r` about the origin in the plane square to
/// `axis`, found at every degree of the circle, for which `keep` holds of the point there.
fn circle_edges(b: &Body, r: f64, axis: usize, keep: impl Fn([f64; 3]) -> bool) -> Vec<usize> {
    let (k, brep) = kernel_copy(b);
    let all = brep.edge_list();
    let (u, v) = [(1, 2), (0, 2), (0, 1)][axis];
    let mut edges = Vec::new();
    for i in 0..360 {
        let a = (0.5 + f64::from(i)).to_radians();
        let mut p = [0.0; 3];
        p[u] = r * a.cos();
        p[v] = r * a.sin();
        if !keep(p) {
            continue;
        }
        let q = Vec3::new(p[0], p[1], p[2]);
        if let Some(e) = all.iter().position(|&e| {
            edge_closest_point(&k, &brep, e, q)
                .unwrap()
                .closest
                .distance
                < 1e-9 * r
        }) {
            if !edges.contains(&e) {
                edges.push(e);
            }
        }
    }
    edges
}

/// Rounds `edges` of `b` by `f` and checks the result: one valid manifold solid whose volume
/// changed by `want`. Both volumes are integrated to `RIM_VOLUME_ACCURACY` of themselves, so
/// the change is known to the sum of those two bounds; the ring's closed form is exact to
/// rounding. Returns the change.
fn round_rim(case: &str, b: &Body, edges: &[usize], f: f64, want: Option<f64>) -> f64 {
    let before = b.volume(RIM_VOLUME_ACCURACY).unwrap();
    blend(
        b,
        Blend::Round,
        edges,
        &vec![f; edges.len()],
        &[],
        Corners::Filled,
    )
    .unwrap_or_else(|e| panic!("{case}: the round does not build: {e:?}"));
    assert!(b.is_manifold_solid().unwrap(), "{case}");
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{case}: {report}");
    let after = b.volume(RIM_VOLUME_ACCURACY).unwrap();
    let change = after - before;
    if let Some(want) = want {
        let bound = RIM_VOLUME_ACCURACY * (before.abs() + after.abs()) + 1e-14 * want.abs();
        assert!(
            (change - want).abs() <= bound,
            "{case}: the volume changed by {change}, not {want} ({:.2e} relative, the \
             integration allows {:.2e})",
            (change - want).abs() / want.abs(),
            bound / want.abs()
        );
    }
    change
}

/// Rounds the whole rim of a sphere of radius `r` cut or joined with its centre on a plate's
/// face (see [`sphere_on_plate`]) by `f`, and checks the volume against the ring's closed form.
fn round_sphere_rim(
    case: &str,
    r: f64,
    f: f64,
    axis: usize,
    side: f64,
    moves: &[[[f64; 4]; 4]],
    op: solidcore_ops::BooleanOp,
) {
    let b = sphere_on_plate(r, axis, side, moves, op);
    let edges = circle_edges(&b, r, axis, |_| true);
    assert!(!edges.is_empty(), "{case}: no edge on the rim");
    let ring = sphere_on_plane_round(r, f);
    let want = if op == solidcore_ops::BooleanOp::Union {
        ring
    } else {
        -ring
    };
    round_rim(case, &b, &edges, f, Some(want));
}

/// The nub and socket cases each rim-round test runs: (`op`, the plate's side).
const NUB_AND_SOCKET: [(solidcore_ops::BooleanOp, f64); 3] = [
    (solidcore_ops::BooleanOp::Union, 1.0),
    (solidcore_ops::BooleanOp::Union, -1.0),
    (solidcore_ops::BooleanOp::Subtract, 1.0),
];

/// tangency-labs/solidcraft#18829 and #18826: a round where a sphere meets a flat face it is
/// centred on, as a nub (joined) or a socket (cut), with the face square to each axis. The
/// sphere's poles lie on its z axis, so on a face square to x or y the rim runs through both
/// poles and is split into its edges there: at each pole two pieces of the round meet as one
/// blend, solved on one section from the ball's feet. The feet were the vertex's parameters
/// stepped by the binormal's first-order image, which at a pole is unbounded, and the round
/// failed ("Fillet Corner Error: Unable to Compute Geometry"). They are now the sphere's points
/// nearest the vertex stepped along the binormal, found in 3D. Turning the sphere about z moves
/// its seam and nothing else. Every round builds a valid solid and adds or takes off the
/// ring's closed form, to the volume integration's accuracy.
#[test]
fn a_round_where_a_sphere_meets_a_face_through_its_poles_builds() {
    use solidcore_ops::BooleanOp;
    all_fixes();
    let (r, f) = (0.975, 0.3);
    for (axis, side, turn, op) in [
        (2, -1.0, 0.0, BooleanOp::Union),
        (1, 1.0, 0.0, BooleanOp::Union),
        (1, -1.0, 0.0, BooleanOp::Union),
        (1, 1.0, 30.0, BooleanOp::Union),
        (1, 1.0, 180.0, BooleanOp::Union),
        (0, 1.0, 0.0, BooleanOp::Union),
        (0, -1.0, 0.0, BooleanOp::Union),
        (1, 1.0, 0.0, BooleanOp::Subtract),
        (1, -1.0, 0.0, BooleanOp::Subtract),
        (0, 1.0, 0.0, BooleanOp::Subtract),
    ] {
        let case =
            format!("face square to axis {axis}, plate on {side}, sphere turned {turn}, {op:?}");
        round_sphere_rim(&case, r, f, axis, side, &[turn_about(2, turn)], op);
    }
}

/// #18826's socket: a sphere of radius 1 cut with its centre on a block's face, its rim
/// rounded by 0.1, 0.2 and 0.3, and the same as a nub; then the reported part, with a
/// vertical channel (radius 0.9, its axis 0.35 outside the face) cut across the socket's top,
/// which takes the rim's top from x = -0.829 to 0.829. The channelled round has no closed
/// form; it must build a valid solid that takes less than the whole ring.
#[test]
fn a_round_on_a_rim_through_a_spheres_poles_joins_at_the_poles() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::{make_sphere, BooleanOp};
    all_fixes();
    for (op, side) in NUB_AND_SOCKET {
        for f in [0.1, 0.2, 0.3] {
            round_sphere_rim(
                &format!("{op:?} on {side}, f {f}"),
                1.0,
                f,
                1,
                side,
                &[],
                op,
            );
        }
    }
    let block = make_box([-5.0, 0.0, 0.0], [10.0, 5.0, 10.0]).unwrap();
    let channel = make_cylinder([0.0, -0.35, 5.0], 0.9, 6.0).unwrap();
    let b = boolean(block, channel, BooleanOp::Subtract)
        .unwrap()
        .unwrap();
    let socket = make_sphere([0.0, 0.0, 5.0], 1.0).unwrap();
    let b = boolean(b, socket, BooleanOp::Subtract).unwrap().unwrap();
    let half_width = (0.81_f64 - 0.35 * 0.35).sqrt();
    let (k, brep) = kernel_copy(&b);
    let mut edges = Vec::new();
    for degrees in [20.0_f64, -45.0, -135.0, 200.0] {
        let t = degrees.to_radians();
        let q = Vec3::new(t.cos(), 0.0, 5.0 + t.sin());
        assert!(t.sin() < 0.0 || t.cos().abs() > half_width);
        let e = brep
            .edge_list()
            .iter()
            .position(|&e| {
                edge_closest_point(&k, &brep, e, q)
                    .unwrap()
                    .closest
                    .distance
                    < 1e-9
            })
            .unwrap();
        if !edges.contains(&e) {
            edges.push(e);
        }
    }
    assert_eq!(edges.len(), 3, "the channelled rim's edges");
    let taken = -round_rim("the channelled socket", &b, &edges, 0.2, None);
    assert!(
        taken > 0.0 && taken < sphere_on_plane_round(1.0, 0.2),
        "the channelled socket's round took {taken}"
    );
}

/// The poles' rounding noise grows with the square of the radius (|du x dv| at an exact pole
/// is 6e-11 at radius 0.975 and 1.3e-8 at 20), so a test of the parameterisation's singularity
/// against a fixed limit lets the first-order step back in from radius 17 or so up. The foot
/// is found in 3D at every size: the rim of a sphere of radius 0.01 to 100 builds, nub and
/// socket, with the round a fixed share of the radius.
#[test]
fn a_round_on_a_sphere_s_rim_through_its_poles_builds_at_any_size() {
    all_fixes();
    for r in [0.01, 20.0, 40.0, 100.0] {
        for (op, side) in NUB_AND_SOCKET {
            let case = format!("radius {r}, {op:?} on {side}");
            round_sphere_rim(&case, r, 0.3 * r / 0.975, 1, side, &[], op);
        }
    }
}

/// A sphere tilted about x by 1e-9 to 1e-6 radians: its poles leave the rim by that much, and
/// the rim's vertices sit beside them, where the sphere's parameterisation is nearly singular.
/// The first-order step there was up to 5e7 in longitude; the 3D foot is unaffected.
#[test]
fn a_round_on_a_sphere_s_rim_with_its_poles_beside_the_rim_builds() {
    all_fixes();
    for tilt in [1e-9_f64, 1e-8, 1e-7, 1e-6] {
        for seam in [0.0, 90.0] {
            for (op, side) in NUB_AND_SOCKET {
                let case = format!("tilt {tilt:e} rad, seam turned {seam}, {op:?} on {side}");
                let moves = [turn_about(2, seam), turn_about(0, tilt.to_degrees())];
                round_sphere_rim(&case, 0.975, 0.3, 1, side, &moves, op);
            }
        }
    }
}

/// A sphere turned about the face's normal: its poles stay on the rim, at other places. At 5
/// degrees a rim vertex lies 5 degrees from a pole, where the parameterisation is regular
/// (|du x dv| = 1.37) but the first-order step of -0.62 in u carried the foot across the pole
/// onto the wrong meridian. With the seam turned 200 degrees a join lies on the sphere's seam,
/// and the foot found in 3D must keep to the edge use's side of it (u = 1.0086, not 0.0086).
#[test]
fn a_round_on_a_sphere_s_rim_turned_about_the_face_normal_builds() {
    all_fixes();
    for turn in [5.0, 30.0, 137.0] {
        for seam in [0.0, 200.0] {
            for (op, side) in NUB_AND_SOCKET {
                let case = format!("turned {turn} about y, seam turned {seam}, {op:?} on {side}");
                let moves = [turn_about(2, seam), turn_about(1, turn)];
                round_sphere_rim(&case, 0.975, 0.3, 1, side, &moves, op);
            }
        }
    }
}

/// Half a capsule: the cylinder of radius 5 from z = 0 to 10 joined with the spheres of radius
/// 5 about its ends' centres, less everything below y = 0. The cut face's outline is one
/// closed tangent chain: the lines x = 5 and x = -5 along the cylinder and, on each sphere, the
/// half circle in y = 0, which the spheres' poles (on the z axis) split into two quarters.
fn half_capsule() -> Body {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::{make_sphere, BooleanOp};
    let c = make_cylinder([0.0; 3], 5.0, 10.0).unwrap();
    let b = boolean(c, make_sphere([0.0; 3], 5.0).unwrap(), BooleanOp::Union)
        .unwrap()
        .unwrap();
    let b = boolean(
        b,
        make_sphere([0.0, 0.0, 10.0], 5.0).unwrap(),
        BooleanOp::Union,
    )
    .unwrap()
    .unwrap();
    let cut = make_box([-10.0, -10.0, -10.0], [20.0, 10.0, 30.0]).unwrap();
    boolean(b, cut, BooleanOp::Subtract).unwrap().unwrap()
}

/// The section a blend of offsets `plane` (from the cut face y = 0) and `round` (from the
/// curved face) takes off where y = 0 meets a circle of radius 5 about the origin, the body
/// being the disc's half y >= 0: its area and its first moment about x = 0. The point at those
/// offsets is c = (sqrt((5 - round)^2 - plane^2), plane); its feet are (c.x, 0) on the plane
/// and 5 c / |c| on the circle, at angle `th`. A chamfer takes the triangle of the edge (5, 0)
/// and the feet, with the circle's segment between angles 0 and `th`; a round takes that
/// less the ball's segment its two feet cut off (from angle -pi/2 to `th` about c). Every
/// section of the half capsule's outline is this one: along a line in its plane z = const,
/// along an arc in the half plane through its sphere's centre and the y axis, x being the
/// distance from that axis.
fn capsule_section(plane: f64, round: f64, ball: bool) -> (f64, f64) {
    let r = 5.0_f64;
    // a circle's segment between angles a0 and a1: area, and its centroid's x about the centre
    let segment = |radius: f64, a0: f64, a1: f64| {
        let t = a1 - a0;
        let area = 0.5 * radius * radius * (t - t.sin());
        let d = 4.0 * radius * (0.5 * t).sin().powi(3) / (3.0 * (t - t.sin()));
        (area, d * (0.5 * (a0 + a1)).cos())
    };
    let cx = ((r - round).powi(2) - plane * plane).sqrt();
    let s = r / (r - round);
    let foot = (cx * s, plane * s);
    let th = foot.1.atan2(foot.0);
    let triangle = 0.5 * (r - cx) * foot.1;
    let (sa, sx) = segment(r, 0.0, th);
    let mut area = triangle + sa;
    let mut moment = triangle * (r + cx + foot.0) / 3.0 + sa * sx;
    if ball {
        let (ba, bx) = segment(plane, -0.5 * PI, th);
        area -= ba;
        moment -= ba * (cx + bx);
    }
    (area, moment)
}

/// The half capsule's volume: half the cylinder and half of each of two hemispheres.
const HALF_CAPSULE: f64 = 0.5 * PI * 25.0 * 10.0 + 2.0 * PI * 125.0 / 3.0;

/// An unequal chamfer of the half capsule's whole outline, the first size along the cut face,
/// was refused (then reported as "the chamfer size", status 1007). The chain is measured at its narrowest
/// wedge, a right angle all round, and lifted to where unequal sizes do not fold, from each
/// edge's widest wedge, which is bounded by its faces' normals enclosed along it. On a quarter
/// arc that ends at a sphere's pole the normal `S_u x S_v` vanishes there, so the piece of the
/// arc that holds the pole bounds no cosine however often it is halved: the bound was a half
/// turn, and a chamfer at a wedge of pi has no definition. A sphere's normal is now enclosed
/// by its definition, `S - c`, and every arc is bounded at a right angle. At that wedge the
/// offsets are the setbacks swapped (`ChamferDefinition::offsets`): the cut face is offset by
/// the second size and the curved faces by the first. The chamfer takes off the section of
/// [`capsule_section`] along the two lines (10 each) and, by Pappus, turned through pi about
/// each sphere's centre.
#[test]
fn an_unequal_chamfer_of_a_half_capsule_s_outline_is_measured_at_a_right_angle() {
    use solidcore::blend::chamfer_side::widest_wedge_bound;
    all_fixes();
    let b = half_capsule();
    let line = edge_at(&b, [5.0, 0.0, 5.0]);
    let chain = tangent_chain(&b, None, &[line], &[], 10f64.to_radians())
        .unwrap()
        .members;
    assert_eq!(chain.len(), 6, "two lines and four quarter arcs");
    let (k, mut brep) = kernel_copy(&b);
    let edges = brep.edge_list().to_vec();
    for &e in &chain {
        let w = widest_wedge_bound(&k, &mut brep, edges[e], 1e-9).unwrap();
        assert!(
            w.wedge >= 0.5 * PI && w.wedge - 0.5 * PI < 1e-6,
            "edge {e}: {w:?}"
        );
    }
    for (flat, round) in [(2.0, 1.0), (1.0, 2.0)] {
        let b = half_capsule();
        let chain = tangent_chain(&b, None, &[line], &[], 10f64.to_radians())
            .unwrap()
            .members;
        let along: Vec<usize> = chain
            .iter()
            .map(|&e| {
                let [(f0, l0), (f1, _)] = edge_faces_along(&b, e, [0.0, -1.0, 0.0]).unwrap();
                if l0 > 0.5 {
                    f0
                } else {
                    f1
                }
            })
            .collect();
        chamfer_sides(&b, &chain, &along, flat, ChamferSecond::Distance(round)).unwrap();
        assert!(
            b.check(CheckLevel::All).unwrap().is_valid(),
            "{flat}/{round}"
        );
        let (area, moment) = capsule_section(round, flat, false);
        let want = HALF_CAPSULE - 20.0 * area - 2.0 * PI * moment;
        assert!(
            rel(volume(&b), want) < 1e-9,
            "{flat}/{round}: {} against {want}",
            volume(&b)
        );
    }
}

/// #17594's slab: the top 1.206 of a hexagonal boss whose six upright corners are rounds of
/// radius 1 (axes on the circle of radius `HEX_RC` about (0, -69.5), top z = 30.706, bottom
/// z = 29.5), cut from the imported part as SolidCraft traced it (`b1`).
fn hex_slab() -> Body {
    let case = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/issue_17594_hex_slab.bodies.json.gz"
    );
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(case).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let records: serde_json::Value = serde_json::from_str(&text).unwrap();
    let record: Record = serde_json::from_value(records["b1"].clone()).unwrap();
    Body::from_record(&record, true).unwrap().0
}

/// The radius of the circle the slab's corner axes stand on: 2 x 5.928203230275 / sqrt(3).
const HEX_RC: f64 = 6.845_299_461_620_161;

/// #17594: the slab's top rim (six straight edges and the six corner rounds' arcs, one
/// tangent chain) rounded at the corners' own radius, r 1. Along each arc the ball touches the
/// corner's cylinder all round its equator, so its centre stays on the corner's axis: the
/// piece is a sphere of radius 1 about the axis 1 below the top (the round's arc turned about
/// the axis, a torus with no hole). Each
/// straight round removes `(1 - pi/4) HEX_RC` (its section over the side between the axes),
/// each corner `pi/6 - pi/9 = pi/18` (the 60 degree sector of the unit cylinder 1 high less
/// that of the upper half ball). On main, at the one corner whose round has the top as its
/// first face, the turn was taken from the top's rail point, the ball's pole on the axis, and
/// the section to the next straight round came out a curve up to 0.61 off its meridian and
/// 0.21 off that round's cylinder: the check finds that gap, and the volume is off where the
/// faces miss each other (the slab lost 9.674 for 9.861; the whole part gained 5.85).
#[test]
fn a_hex_rim_rounded_at_its_corner_rounds_radius_closes_on_six_spheres() {
    rig_fixes();
    let b = hex_slab();
    let before = b.volume(1e-11).unwrap();
    let top = 30.706;
    let corners: Vec<[f64; 2]> = (0..6)
        .map(|i| {
            let a = (90.0 - 60.0 * f64::from(i)).to_radians();
            [HEX_RC * a.cos(), -69.5 + HEX_RC * a.sin()]
        })
        .collect();
    // the arc about corner `i`: its middle, one out from the axis, away from the centre
    let arc = |i: usize| {
        let c = corners[i];
        let (dx, dy) = (c[0], c[1] + 69.5);
        nearest_edge(&b, [c[0] + dx / HEX_RC, c[1] + dy / HEX_RC, top])
    };
    // the side from corner `i` to the next: its middle, one out from the axes' midpoint
    let side = |i: usize| {
        let (c, n) = (corners[i], corners[(i + 1) % 6]);
        let m = [0.5 * (c[0] + n[0]), 0.5 * (c[1] + n[1])];
        let (mx, my) = (m[0], m[1] + 69.5);
        let len = mx.hypot(my);
        nearest_edge(&b, [m[0] + mx / len, m[1] + my / len, top])
    };
    // in SolidCraft's order: the picked side x = 6.928, then its tangent chain outward both
    // ways (the order the chain's start and its corners are taken in)
    let rim = [
        side(1),
        arc(1),
        arc(2),
        side(0),
        side(2),
        arc(0),
        arc(3),
        side(5),
        side(3),
        arc(5),
        arc(4),
        side(4),
    ];
    let mut distinct = rim.to_vec();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), 12, "the rim's twelve edges");
    blend(&b, Blend::Round, &rim, &[1.0], &[], Corners::Filled).unwrap();
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    // the slab's 14 faces and the twelve rounds; the sides' 12 upright edges, the bottom's
    // 12, the rounds' 12 contacts with the walls, 6 on the top and their 12 sections; the
    // bottom's 12 vertices, the contacts' 12 ends and the six poles on the top
    assert_eq!(b.counts().unwrap(), [26, 54, 30]);
    // the top, the bottom and the six sides; the six corner walls and the six straight
    // rounds, cylinders; the six corner pieces, each the arc turned about its corner's axis
    // (26 in all: no face of another kind)
    let (_, brep) = kernel_copy(&b);
    for &f in brep.face_list() {
        if let SurfaceKind::Cone(c) = brep.face(f).unwrap().surface.as_ref().unwrap().kind() {
            assert!(c.is_cylinder(), "face {f:?} on a cone");
        }
    }
    let count = |name: &str| {
        brep.face_list()
            .iter()
            .filter(|&&f| {
                surface_kind_name(brep.face(f).unwrap().surface.as_ref().unwrap()) == name
            })
            .count()
    };
    assert_eq!(
        [
            count("plane"),
            count("cone"),
            count("surface-of-revolution")
        ],
        [8, 12, 6]
    );
    // each surface of revolution is the ball about its own corner's axis point 1 below the
    // top: a 7 x 7 grid over its face's domain lies on that unit sphere
    let (k, _) = kernel_copy(&b);
    let mut at_corner = Vec::new();
    for &f in brep.face_list() {
        let s = brep.face(f).unwrap().surface.as_ref().unwrap();
        if surface_kind_name(s) != "surface-of-revolution" {
            continue;
        }
        let d = solidcore::topology::face::uv_domain(&brep, f).unwrap();
        let mid = s
            .evaluate_point(
                &k,
                Point2::new(0.5 * (d.u_min() + d.u_max()), 0.5 * (d.v_min() + d.v_max())),
            )
            .unwrap();
        let (i, c) = corners
            .iter()
            .enumerate()
            .min_by(|a, b| {
                let da = (a.1[0] - mid.x).hypot(a.1[1] - mid.y);
                let db = (b.1[0] - mid.x).hypot(b.1[1] - mid.y);
                da.total_cmp(&db)
            })
            .unwrap();
        at_corner.push(i);
        let centre = Vec3::new(c[0], c[1], top - 1.0);
        for a in 0..7 {
            for e in 0..7 {
                let u = d.u_min() + (d.u_max() - d.u_min()) * f64::from(a) / 6.0;
                let v = d.v_min() + (d.v_max() - d.v_min()) * f64::from(e) / 6.0;
                let p = s.evaluate_point(&k, Point2::new(u, v)).unwrap();
                let off = (p.distance(centre) - 1.0).abs();
                assert!(
                    off <= 1e-9,
                    "face {f:?} at corner {i}: {off:e} off its ball"
                );
            }
        }
    }
    at_corner.sort_unstable();
    assert_eq!(at_corner, [0, 1, 2, 3, 4, 5], "one sphere at each corner");
    let removed = 6.0 * ((1.0 - PI / 4.0) * HEX_RC + PI / 18.0);
    let after = b.volume(1e-11).unwrap();
    assert!(
        (before - after - removed).abs() <= 1e-9 * removed,
        "removed {}, the closed form {removed}",
        before - after
    );
}

/// #19502: a 1 thick wall (x 0…50, z 9…10) joined on a 1 thick plate (x −1…0, z 0…10), the
/// wall's end flush with the plate's side (y = 0) and its top with the plate's top (z = 10).
/// Both long edges of the wall's end, rounded half its thickness, are a full round: the two
/// rounds' rails meet along z = 9.5 from x = 0 to 50, where the lower round ends on the
/// plate, while the top round runs on over the plate's top to x = −1. Each round takes
/// (1 − π/4) r² of the section along its edge (51 and 50 long), so the volume is
/// 1200 − (1 − π/4) r² · 101 at every r up to 0.5, the full round included.
///
/// With a second plate at the wall's other end (x 50…51) the top round runs 52 long and its
/// rail meets the lower one's in its middle third: 1400 − (1 − π/4) r² · 102. Both in either
/// order of the edges.
#[test]
fn a_full_round_whose_rails_meet_along_part_of_one_of_them() {
    solidcore_ops::session::set_thread_fixes(solidcraft_fixes::solidcraft_fix_names()).unwrap();
    // (plates' corners in x, volume before rounding, top edge's length)
    let parts: [(&[f64], f64, f64); 2] = [(&[-1.0], 1200.0, 51.0), (&[-1.0, 50.0], 1400.0, 52.0)];
    for (plates, volume, top) in parts {
        for reversed in [false, true] {
            for r in [0.25, 0.4999, 0.5] {
                let case = format!("plates {plates:?}, reversed {reversed}, r {r}");
                let mut b = make_box([0.0, 0.0, 9.0], [50.0, 20.0, 1.0]).unwrap();
                for &x in plates {
                    let plate = make_box([x, 0.0, 0.0], [1.0, 20.0, 10.0]).unwrap();
                    b = solidcore_ops::boolean::boolean(b, plate, solidcore_ops::BooleanOp::Union)
                        .unwrap()
                        .unwrap();
                }
                let mut edges = [
                    edge_at(&b, [25.0, 0.0, 10.0]),
                    edge_at(&b, [25.0, 0.0, 9.0]),
                ];
                if reversed {
                    edges.reverse();
                }
                blend(&b, Blend::Round, &edges, &[r, r], &[], Corners::Filled)
                    .unwrap_or_else(|e| panic!("{case}: {e:?}"));
                let report = b.check(CheckLevel::All).unwrap();
                assert!(report.is_valid(), "{case}: {report}");
                let want = volume - (1.0 - PI / 4.0) * r * r * (top + 50.0);
                let got = b.volume(1e-11).unwrap();
                assert!(
                    (got - want).abs() <= 1e-9 * want,
                    "{case}: {got} vs closed form {want}"
                );
            }
        }
    }
}

/// The half capsule's outline from pole to pole (the line x = 5 and its two quarter arcs) is
/// one chain that ends at the spheres' poles, where it runs on into the arcs left sharp. A
/// chamfer or a round of it failed ("Fillet Surface Computation Failure", status 1001): the
/// end's section was solved from a guess whose step off the edge was the binormal's
/// first-order image in the sphere's parameters, which is 0 at a pole, so the solve started on
/// the pole, where the longitude is undetermined, and found none. The step is now taken in
/// 3D. Each end is the section in the plane through the pole square to the edge, so the blend
/// takes off [`capsule_section`] along the line (10) and, by Pappus, turned through pi/2 about
/// each sphere's centre. An equal chamfer of 1 has offsets 1 at a right angle.
#[test]
fn a_chamfer_or_round_of_a_half_capsule_s_outline_from_pole_to_pole_builds() {
    all_fixes();
    let pick = |b: &Body| {
        [
            edge_at(b, [3.0, 0.0, -4.0]),
            edge_at(b, [5.0, 0.0, 5.0]),
            edge_at(b, [3.0, 0.0, 14.0]),
        ]
    };
    for ball in [false, true] {
        let b = half_capsule();
        let edges = pick(&b);
        let kind = if ball { Blend::Round } else { Blend::Chamfer };
        blend(&b, kind, &edges, &[1.0], &[], Corners::Filled).unwrap();
        assert!(b.check(CheckLevel::All).unwrap().is_valid(), "{kind:?}");
        let (area, moment) = capsule_section(1.0, 1.0, ball);
        let want = HALF_CAPSULE - 10.0 * area - PI * moment;
        assert!(
            rel(volume(&b), want) < 1e-9,
            "{kind:?}: {} against {want}",
            volume(&b)
        );
    }
}

/// The closed-form volume of a cylinder of radius 12.5 and height 50 on the origin, joined
/// to a box 50 x 25 x 25 high that runs from the cylinder's axis along x, its side at
/// y = 12.5 in the cylinder's tangent plane and its other side `d` inside the tangent plane
/// at y = -12.5 (outside for a negative `d`), with a round of radius 2 on the edge between
/// the box's top and the cylinder.
///
/// It is the cylinder plus the box, less what they share (half the cylinder's disc under
/// the top, less half the segment the near side cuts off it), plus the round. The round
/// fills, over the top, the spandrel between the cylinder, the top and the ball: at the
/// radius `rho` from the axis, from `R` to `R + r`, its height is `r − sqrt(r² − (R + r −
/// rho)²)`, over the angles the box's top holds there, `asin(R / rho)` towards the tangent
/// side and `asin((R − d) / rho)` towards the other (a right angle where the side is past
/// `rho`). With `rho = R + r − r sin(phi)` the integrand is smooth, and it is summed by
/// Gauss–Legendre's 5-point rule on 400 panels of `phi` (800 panels move it by 1.4e-14).
fn tangent_box_round_volume(d: f64) -> f64 {
    let (big, r, height, top, length) = (12.5_f64, 2.0_f64, 50.0, 25.0, 50.0);
    let lo = big - d;
    let cylinder = PI * big * big * height;
    let block = length * (2.0 * big - d) * top;
    let segment = if d > 0.0 {
        big * big * (lo / big).acos() - lo * (big * big - lo * lo).sqrt()
    } else {
        0.0
    };
    let shared = (PI * big * big / 2.0 - segment / 2.0) * top;
    let spandrel = |phi: f64| {
        let rho = big + r - r * phi.sin();
        let angles = (big / rho).asin() + (lo / rho).min(1.0).asin();
        angles * rho * r * (1.0 - phi.cos()) * r * phi.cos()
    };
    let nodes = [
        (0.0, 0.568_888_888_888_888_9),
        (0.538_469_310_105_683_1, 0.478_628_670_499_366_47),
        (-0.538_469_310_105_683_1, 0.478_628_670_499_366_47),
        (0.906_179_845_938_664, 0.236_926_885_056_189_08),
        (-0.906_179_845_938_664, 0.236_926_885_056_189_08),
    ];
    let panels = 400_u32;
    let h = PI / 2.0 / f64::from(panels);
    let round: f64 = (0..panels)
        .map(|i| {
            let mid = (f64::from(i) + 0.5) * h;
            nodes
                .iter()
                .map(|&(x, w)| w * spandrel(mid + 0.5 * h * x))
                .sum::<f64>()
        })
        .sum::<f64>()
        * 0.5
        * h;
    cylinder + block - shared + round
}

/// A round ending on a side a hair off its cylinder's tangent plane (#20421). The box of
/// [`tangent_box_round_volume`] joined to its cylinder with one side `d` inside the
/// cylinder's tangent plane, or outside it, up to what the join holds as one seam at the
/// touch (the contact rule: 1e-5, and 1.1e-5 outside); the 2 mm round on the edge between
/// the box's top and the cylinder ends on that side at a tangent end the side stands `d`
/// off. The rib there is the rib of that standoff
/// (`blend::fillet_geom::tangent_end_rib::Standoff`): against the surfaces as they are no
/// curve from the tangent end is certified, and the round was refused as not certified
/// from 1e-10 on (it built before its rib was certified, up to 1e-7, and failed with a bare
/// status from 1e-6). Each row builds one closed, valid solid facing out with the closed-form
/// volume within 1e-9 relative (the round's end moves by at most the standoff, 1e-5 over an
/// end face under 15 mm²: 3e-9 of the volume at most, measured under 6e-11).
#[test]
fn a_round_ends_on_a_side_a_hair_off_its_cylinders_tangent_plane() {
    rig_fixes();
    for d in [
        0.0, 1e-10, 1e-9, 1e-7, 1e-6, 9e-6, 1e-5, -1e-9, -1e-7, -1e-6, -9e-6, -1.1e-5,
    ] {
        let cylinder = make_cylinder([0.0; 3], 12.5, 50.0).unwrap();
        let block = make_box([0.0, -(12.5 - d), 0.0], [50.0, 25.0 - d, 25.0]).unwrap();
        let b = solidcore_ops::boolean::boolean(cylinder, block, solidcore_ops::BooleanOp::Union)
            .unwrap()
            .unwrap();
        let e1 = nearest_edge(&b, [8.838_834_76, 8.838_834_76, 25.0]);
        let e2 = nearest_edge(&b, [8.838_834_76, -8.838_834_76, 25.0]);
        assert_ne!(e1, e2, "{d:e}");
        blend(
            &b,
            Blend::Round,
            &[e1, e2],
            &[2.0, 2.0],
            &[],
            Corners::Filled,
        )
        .unwrap_or_else(|e| panic!("{d:e}: {e:?}"));
        let report = b.check(CheckLevel::All).unwrap();
        assert!(report.is_valid(), "{d:e}: {}", report.text);
        assert!(b.is_manifold_solid().unwrap(), "{d:e}");
        b.check_solid().unwrap();
        let (v, want) = (b.volume(1e-10).unwrap(), tangent_box_round_volume(d));
        assert!(rel(v, want) < 1e-9, "{d:e}: {v} {want}");
    }
}

/// SolidCraft #19967, case 1: the 13 x 24 x 9 block from x = -1 inside the cylinder of radius
/// R = 81.25 about the line x = R, y = 4, which alone bounds it towards x = 0 (no face of the
/// block touches it); the cylinder's edge on the floor (z = 0) chamfered 2, which
/// makes a cone of half-angle 45 degrees, rho = R - 2 + z about that line, then the edge
/// between the end wall y = 24 and that cone chamfered 0.5. The second chamfer's ruled surface
/// is exact but very uneven in speed across its rulings (its rails' weights differ about
/// 58-fold), and the crossing of its end with the floor was not fitted (`sm_SurfacePlaneCurve`
/// ran out of its 20 passes).
///
/// Expected: a valid body and one face made; the chamfered edge runs between two vertices where
/// three faces meet, so the chamfer adds a face, three edges and two vertices: (8, 18, 12) after
/// (7, 15, 10). The volume it takes off is its cross-section, the triangle of legs d = 0.5 at
/// the dihedral angle alpha between the wall and the cone, times the length of the edge, the
/// hyperbola x = R - sqrt((R - 2 + z)^2 - 400) on y = 24 for z in [0, 2]. The cone's
/// outward normal has y-component 20 / rho / sqrt 2 there (0.174 to 0.179), so sin alpha is
/// 0.984 within 1e-3 and the triangle is 0.123; the edge is 2.875 long within 1e-3. The ends
/// cut the prism obliquely: the floor and the cylinder meet the edge at 44 to 46 degrees, and
/// the triangle's centroid lies d sqrt 2 / 3 < 0.24 from the edge, so each end moves the
/// prism's length at the centroid by less than 0.24 / sin 44 degrees = 0.35; the faces' radii
/// (80 and more against d) move the triangle by under 1 %. So the volume taken off lies in
/// 0.123 * (2.875 -/+ 0.7) * (1 -/+ 0.01), within [0.264, 0.445].
#[test]
fn a_chamfer_between_a_wall_and_a_cone_an_earlier_chamfer_made_builds() {
    all_fixes();
    let r = 81.25_f64;
    let b = built(|k| {
        let block = create_box(
            k,
            solidcore::math::Point3::new(-1.0, 0.0, 0.0),
            13.0,
            24.0,
            9.0,
        )?;
        let cyl = create_cylinder(k, solidcore::math::Point3::new(r, 4.0, -1.0), r, 11.0)?;
        boolean_intersection(k, block, cyl)
    });
    // on the floor, the cylinder at y = 12
    let floor_edge = edge_at(&b, [r - (r * r - 64.0).sqrt(), 12.0, 0.0]);
    blend(
        &b,
        Blend::Chamfer,
        &[floor_edge],
        &[2.0],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert_eq!(b.counts().unwrap(), [7, 15, 10]);
    let before = b.volume(1e-10).unwrap();
    // on the wall y = 24, the cone at z = 1 (rho = R - 1)
    let wall_edge = edge_at(&b, [r - ((r - 1.0).powi(2) - 400.0).sqrt(), 24.0, 1.0]);
    let made = blend(
        &b,
        Blend::Chamfer,
        &[wall_edge],
        &[0.5],
        &[],
        Corners::Filled,
    )
    .unwrap();
    assert_eq!(made.made.len(), 1);
    let report = b.check(CheckLevel::All).unwrap();
    assert!(report.is_valid(), "{report}");
    assert_eq!(b.counts().unwrap(), [8, 18, 12]);
    let off = before - b.volume(1e-10).unwrap();
    assert!((0.264..=0.445).contains(&off), "the chamfer took off {off}");
}

/// #19724, the case #19789 did not cover. Rounding the 15° root together with a rib-end
/// edge splits that round. The rib's cone is one 12° span. Read off its tangent leg, the
/// centre misses the axis by more than the reload allows, the support was stored without
/// its cone label, and the split round's net would not certify: the body could not be
/// measured, and a later fillet of the unrelated rim was refused. The chord and the
/// weight keep the cone label, so the body reads back and the rim fillet builds.
#[test]
fn a_round_meeting_a_short_rib_s_end_reads_back() {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::sweep::revolve;
    use solidcore_ops::BooleanOp;
    all_fixes();
    let line = |a: [f64; 3], b: [f64; 3]| Curve::Line { start: a, end: b };
    let read_back = |body: Body, step: &str| {
        let (record, _) = body.to_record().unwrap();
        Body::from_record(&record, true)
            .unwrap_or_else(|e| panic!("{step} does not read back: {e}"))
            .0
    };
    // The frustum (radius 24 at x = 0, 36 at x = 48) and the 12° rib. Each revolve is
    // read back before the join, as a feature's result is.
    let cone = read_back(
        revolve(
            &[
                line([0.0, 0.0, 0.0], [0.0, 0.0, -24.0]),
                line([0.0, 0.0, -24.0], [48.0, 0.0, -36.0]),
                line([48.0, 0.0, -36.0], [48.0, 0.0, 0.0]),
                line([48.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
            ],
            [0.0; 3],
            [1.0, 0.0, 0.0],
            2.0 * PI,
            true,
        )
        .unwrap(),
        "the frustum",
    );
    // The rib profile turned −6° about X, so a 12° revolve is centred on the seam.
    let tri = [
        [18.0, 28.343_874_018, -2.979_061_203],
        [26.743_129, 33.170_913_336, -3.486_403_479],
        [27.370_857, 30.673_754_386, -3.223_941_497],
    ];
    let rib = read_back(
        revolve(
            &[
                line(tri[0], tri[1]),
                line(tri[1], tri[2]),
                line(tri[2], tri[0]),
            ],
            [0.0; 3],
            [1.0, 0.0, 0.0],
            12.0_f64.to_radians(),
            true,
        )
        .unwrap(),
        "the rib",
    );
    let b = read_back(
        boolean(cone, rib, BooleanOp::Union).unwrap().unwrap(),
        "the join",
    );
    // The 15° root, and the two edges where the rib's ends meet the cone (their mids).
    let root = nearest_edge(&b, [18.0, 28.5, 0.0]);
    let end_a = nearest_edge(&b, [22.685_428_5, 29.508_814, -3.101_501]);
    let end_b = nearest_edge(&b, [22.685_428_5, 29.508_814, 3.101_501]);
    assert!(root != end_a && root != end_b && end_a != end_b);
    blend(
        &b,
        Blend::Round,
        &[root, end_a, end_b],
        &[2.0, 2.0, 2.0],
        &[],
        Corners::Filled,
    )
    .unwrap_or_else(|e| panic!("the fillet builds: {e}"));
    let (record, _) = b.to_record().unwrap();
    for face in &record.faces {
        let solidcore_record::brep::Surface::Blend { definition } = &face.surface else {
            continue;
        };
        let solidcore_record::brep::blend::BlendDefinition::ExactRound(round) = definition.as_ref()
        else {
            continue;
        };
        for support in &round.supports {
            let solidcore_record::brep::blend::SupportRecord::Nurbs { kind, .. } = support else {
                continue;
            };
            assert!(
                !matches!(kind, solidcore_record::brep::blend::SupportKindRecord::Free),
                "a round support was stored without its cone label: {kind:?}"
            );
        }
    }
    let b = read_back(b, "the meeting fillet");
    // (0, 0, −24) is also the cone's seam. A quarter turn away is only the sharp rim.
    let rim = edge_at(&b, [0.0, 24.0, 0.0]);
    blend(&b, Blend::Round, &[rim], &[2.0], &[], Corners::Filled)
        .unwrap_or_else(|e| panic!("the rim fillet builds: {e}"));
    read_back(b, "the rim fillet");
}

/// The cylinder of radius 9.5 about +z, 30 high, its seam at `seam` radians from +x (SolidCraft's
/// `solid.cylinder` extrudes one arc starting at 0.6131), less the notch 20 x 8 x 6 across its top
/// on the side of `sign` y: the box x -10..10, |y| 5..13, z 26..32.
fn seamed_notched_cylinder(seam: f64, sign: f64) -> Body {
    use solidcore_ops::boolean::boolean;
    use solidcore_ops::BooleanOp;
    let cylinder = extrude(
        &[Curve::Arc {
            center: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius: 9.5,
            start: seam,
            end: seam + 2.0 * PI,
        }],
        [0.0, 0.0, 1.0],
        30.0,
        0.0,
        DraftCorner::Curvature,
        true,
    )
    .unwrap();
    let y = if sign > 0.0 { 5.0 } else { -13.0 };
    let notch = make_box([-10.0, y, 26.0], [20.0, 8.0, 6.0]).unwrap();
    boolean(cylinder, notch, BooleanOp::Subtract)
        .unwrap()
        .unwrap()
}

/// tangency-labs/solidcraft#21416: the notch's inside edge (y = 5, z = 26) rounded or chamfered
/// 1 builds wherever the cylinder's seam lies. The blend adds material in the notch's corner and
/// ends on the cylinder at both ends; near x > 0 its end curve runs from the wall's rail, at
/// atan(5 / 8.078) = 31.76 degrees, to the floor's, at atan(6 / 7.365) = 39.17 degrees. Where
/// the seam lies between them (SolidCraft's at 35.13 degrees), the end curve crosses it; the
/// intersections traced the curve within the cylinder's domain, which ends at its seam, so none
/// ran from one end to the other, and the blend failed ("failure to calculate intersection of
/// fillet surface with side face"); traced across the seam, the cut of its UV curve beyond the
/// seam lay a period outside the face's domain. The seams tried lie beside each end of the end
/// curve, in its middle, and away from it (near the notch's far end, on the -x side, and a
/// quarter turn off), on the notch at +y and at -y (the mirror image, which built before).
///
/// Expected: a valid, closed solid, whose volume is the closed form: the cylinder, less the
/// notch's circular segment R^2 acos(d/R) - d sqrt(R^2 - d^2) (d = 5) over its 4 mm depth, plus
/// the blend's section at the height and depth (s, t) from the edge times the cylinder's chord
/// there, 2 sqrt(R^2 - (d + s)^2): for the round the section is the square [0, 1]^2 less the
/// ball's quarter disc, so with s = 1 - sin(a) its depth is 1 - cos(a); for the chamfer it is
/// the triangle t < 1 - s. The integrals are smooth and taken by Simpson's rule on 2000 panels,
/// far below the 1e-10 compared (the round's is 8303.852506129, the issue's).
#[test]
fn a_notch_s_round_and_chamfer_end_across_the_cylinder_s_seam() {
    rig_fixes();
    let (big_r, d, r) = (9.5_f64, 5.0_f64, 1.0_f64);
    let simpson = |f: &dyn Fn(f64) -> f64, a: f64, b: f64| {
        let n = 2000;
        let h = (b - a) / f64::from(n);
        let mut s = f(a) + f(b);
        for i in 1..n {
            s += f(a + h * f64::from(i)) * if i % 2 == 1 { 4.0 } else { 2.0 };
        }
        s * h / 3.0
    };
    let chord = |y: f64| 2.0 * (big_r * big_r - y * y).sqrt();
    let notched = PI * big_r * big_r * 30.0
        - 4.0 * (big_r * big_r * (d / big_r).acos() - d * (big_r * big_r - d * d).sqrt());
    let round = notched
        + simpson(
            &|a: f64| chord(d + r - r * a.sin()) * (r - r * a.cos()) * r * a.cos(),
            0.0,
            PI / 2.0,
        );
    let chamfer = notched + simpson(&|y: f64| chord(y) * (d + r - y), d, d + r);
    assert!(rel(round, 8_303.852_506_129) < 1e-12, "{round}");
    let degrees: [f64; 8] = [35.13, 31.9, 32.5, 38.5, 39.0, 120.0, 200.0, 305.0];
    for seam in degrees {
        for sign in [1.0, -1.0] {
            for (kind, want) in [(Blend::Round, round), (Blend::Chamfer, chamfer)] {
                let case = format!("seam {seam} degrees, notch at {sign} y, {kind:?}");
                // the -y notch at the seam's mirror image: the same part mirrored in y = 0
                let at = if sign > 0.0 { seam } else { -seam };
                let b = seamed_notched_cylinder(at.to_radians(), sign);
                let edge = edge_at(&b, [0.0, d * sign, 26.0]);
                blend(&b, kind, &[edge], &[r], &[], Corners::Filled)
                    .unwrap_or_else(|e| panic!("{case}: {e:?}"));
                let report = b.check(CheckLevel::All).unwrap();
                assert!(report.is_valid(), "{case}: {}", report.text);
                b.check_solid().unwrap_or_else(|e| panic!("{case}: {e:?}"));
                let v = b.volume(1e-11).unwrap();
                assert!(rel(v, want) < 1e-10, "{case}: {v} {want}");
            }
        }
    }
}
