//! `SmBrep::DeleteFilletFace` of a four-sided fillet leaves a B-rep the check finds clean:
//! the sharp edge's new curve is owned under its edge's `Brep::owner_identity`, the
//! identity `check_brep` compares (Test #61, `ValidateGeometry(FALSE)`, "Bad Edge"
//! otherwise).
//!
//! The case: a 20 mm cube with its top front edge rounded r 2, the round removed: the
//! sharp cube again, six faces, twelve edges, check clean.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::blend::fillet_removal::{delete_fillet_face, delete_fillet_faces};
use solidcore::boolean::api_brep::{boolean_intersection, boolean_union};
use solidcore::mass::integrator::properties::{brep_area, brep_volume_default};
use solidcore::math::det;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{
    create_box, create_cone, create_cylinder, create_sphere,
};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::query::face::p1::point_on_face;
use solidcore::surface::plane::Plane;
use solidcore::topology::brep::is_outward_solid;
use solidcore::topology::brep::mirror::transform_matrix;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::face::p2 as face2;
use solidcore::topology::{edge, vertex, Brep, FaceId};
use solidcore::{Kernel, Transform};
use std::f64::consts::PI;

#[test]
fn removing_a_four_sided_round_leaves_a_clean_brep() {
    let mut k = Kernel::default();
    let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 20.0, 20.0, 20.0).unwrap();
    let e = b
        .edges()
        .ids()
        .find(|&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            [s, t].into_iter().flatten().all(|v| {
                let p = vertex::point(&b, v).unwrap();
                p.y == 0.0 && p.z == 20.0
            })
        })
        .unwrap();
    fillet_edges(&mut k, &mut b, &[e], 2.0, 1, 1, 1.0).unwrap();
    let round = *b
        .face_list()
        .iter()
        .find(|&&f| {
            !face2::surface(&b, f)
                .unwrap()
                .unwrap()
                .is_kind_of(Plane::TYPE)
                .unwrap()
        })
        .unwrap();
    b.set_editing_enabled(true);
    delete_fillet_face(&mut k, &mut b, round, None).unwrap();
    assert_eq!(b.face_list().len(), 6);
    assert_eq!(b.edges().ids().count(), 12);
    assert!(brep_is_manifold_solid(&b).unwrap());
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
}

/// Round the edges whose two ends and middle satisfy `on`. Distinct split arcs can
/// have the same endpoints as another edge, such as a sphere's meridian.
fn round_edges(k: &mut Kernel, b: &mut Brep, radius: f64, on: impl Fn([Point3; 3]) -> bool) {
    let edges: Vec<_> = b
        .edges()
        .ids()
        .filter(|&e| {
            let (s, t) = edge::start_end_vertices(b, e).unwrap();
            let i = edge::interval(b, e).unwrap();
            let c = edge::curve(b, e).unwrap().unwrap();
            let mid = c.evaluate_point(k, (i.min() + i.max()) * 0.5).unwrap();
            on([
                vertex::point(b, s.unwrap()).unwrap(),
                mid,
                vertex::point(b, t.unwrap()).unwrap(),
            ])
        })
        .collect();
    fillet_edges(k, b, &edges, radius, 1, 1, 1.0).unwrap();
}

/// A radius-one round bridges two heights within one diameter of the rim.
/// The flat caps have one height, and each original wall reaches at least
/// two units away from the rim.
fn round_faces(b: &Brep, z: f64) -> Vec<FaceId> {
    b.face_list()
        .iter()
        .copied()
        .filter(|&f| {
            let vertices =
                solidcore::topology::face::p4::vertices(b, f, None, false, None).unwrap();
            let (lo, hi) = vertices
                .iter()
                .map(|&v| vertex::point(b, v).unwrap().z)
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), h| {
                    (lo.min(h), hi.max(h))
                });
            hi - lo > 1e-7 && lo > z - 2.0 + 1e-7 && hi < z + 2.0 - 1e-7
        })
        .collect()
}

fn spherical_cap(k: &mut Kernel, center_z: f64, bottom: f64) -> Brep {
    let sphere = create_sphere(k, Point3::new(0.0, 0.0, center_z), 5.0).unwrap();
    let clip = create_box(k, Point3::new(-6.0, -6.0, bottom), 12.0, 12.0, 6.0).unwrap();
    boolean_intersection(k, sphere, clip).unwrap()
}

/// A rotation of the top body before the union puts its seam at a different
/// longitude from the lower body's. Their common rim then has two edges.
fn turned(k: &mut Kernel, mut body: Brep, angle: f64) -> Brep {
    let m = Transform::rotation(Point3::ZERO, Point3::new(0.0, 0.0, 1.0), angle).unwrap();
    transform_matrix(k, &mut body, &m.m, true).unwrap();
    body
}

fn faces_at(k: &Kernel, body: &mut Brep, p: Point3) -> usize {
    let faces = body.face_list().to_vec();
    faces
        .into_iter()
        .filter(|&f| point_on_face(k, body, f, p, 1e-7).unwrap().is_some())
        .count()
}

fn mass(k: &Kernel, b: &mut Brep) -> [f64; 2] {
    [
        brep_volume_default(k, b, 1e-10).unwrap(),
        brep_area(k, b, 1e-10).unwrap(),
    ]
}

fn removes_chain(
    k: &mut Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    crest: &[Point3],
    expected: [f64; 2],
) {
    let remaining = b.face_list().len() - faces.len();
    assert!((mass(k, b)[0] - expected[0]).abs() > 0.01);
    for &p in crest {
        assert_eq!(faces_at(k, b, p), 0, "rounded input at {p:?}");
    }
    b.set_editing_enabled(true);
    delete_fillet_faces(k, b, faces, None).unwrap();
    assert_eq!(b.face_list().len(), remaining);
    assert!(brep_is_manifold_solid(b).unwrap());
    assert!(is_outward_solid(k, b).unwrap());
    let issues = check_brep(k, b, CheckLevel::Level2).unwrap().lines();
    assert!(issues.is_empty(), "{issues:?}");
    for (got, want) in mass(k, b).into_iter().zip(expected) {
        assert!((got - want).abs() <= 1e-7, "got {got}, expected {want}");
    }
    for &p in crest {
        assert_eq!(faces_at(k, b, p), 2, "sharp rim at {p:?}");
    }
}

fn removes_split_ring(k: &mut Kernel, mut b: Brep, r: f64, z: f64, expected: [f64; 2]) {
    round_edges(k, &mut b, 1.0, |p| {
        p.iter()
            .all(|p| (p.z - z).abs() < 1e-7 && (det::hypot(p.x, p.y) - r).abs() < 1e-7)
    });
    let mut faces = round_faces(&b, z);
    assert_eq!(faces.len(), 2);
    faces.reverse();
    let crest = [0.41_f64, 1.9, 3.8, 5.7]
        .map(|theta| Point3::new(r * det::cos(theta), r * det::sin(theta), z));
    removes_chain(k, &mut b, &faces, &crest, expected);
}

#[test]
fn removes_a_split_toroidal_round_from_a_hemisphere() {
    let mut k = Kernel::default();
    let b = spherical_cap(&mut k, 0.0, 0.0);
    removes_split_ring(&mut k, b, 5.0, 0.0, [250.0 * PI / 3.0, 75.0 * PI]);
}

/// One round patch and its neighbour share two cross edges. Its other two
/// edges are the rails to be sewn together on the plane and on the sphere.
fn hemisphere_with_rails(k: &mut Kernel) -> (Brep, Vec<FaceId>, [solidcore::EdgeId; 2]) {
    let mut b = spherical_cap(k, 0.0, 0.0);
    round_edges(k, &mut b, 1.0, |p| p.iter().all(|p| p.z.abs() < 1e-7));
    let patches = round_faces(&b, 0.0);
    let rails: Vec<_> = solidcore::topology::face::p4::edges(&b, patches[0], None, false, None)
        .unwrap()
        .into_iter()
        .filter(|&e| {
            edge::faces(&b, e, None)
                .unwrap()
                .iter()
                .any(|f| !patches.contains(f))
        })
        .collect();
    b.set_editing_enabled(true);
    (b, patches, rails.try_into().unwrap())
}

#[test]
fn both_sides_of_a_sharp_rail_keep_the_tighter_source_tolerance() {
    let mut k = Kernel::default();
    let (b, patches, rails) = hemisphere_with_rails(&mut k);
    for loose in rails {
        let mut copy = b.clone_standing_for_self().unwrap();
        let budget = edge::tolerance(&copy, loose).unwrap();
        edge::set_tolerance(&mut copy, loose, 10.0 * budget, false, true).unwrap();
        assert!(check_brep(&mut k, &mut copy, CheckLevel::Level2)
            .unwrap()
            .is_valid());
        delete_fillet_faces(&mut k, &mut copy, &patches, None).unwrap();
        let surviving: Vec<_> = rails
            .iter()
            .copied()
            .filter(|&e| copy.edge(e).is_ok())
            .collect();
        assert_eq!(surviving.len(), 1);
        assert_eq!(edge::tolerance(&copy, surviving[0]).unwrap(), budget);
    }
}

#[test]
fn a_sharp_rail_trim_without_a_certificate_restores_the_original_rails() {
    use solidcore::error::blend::{BlendFailure, RemovalRefusal};
    use solidcore::error::Reason;
    let mut k = Kernel::default();
    let (mut b, patches) = rounded_block(&mut k);
    b.set_editing_enabled(true);
    // The straight rail along the front wall joins (3, 0, 7) to (17, 0, 7).
    let tight = *b
        .edge_list()
        .iter()
        .find(|&&e| {
            let (s, t) = edge::start_end_vertices(&b, e).unwrap();
            [s.unwrap(), t.unwrap()].into_iter().all(|v| {
                let p = vertex::point(&b, v).unwrap();
                p.y.abs() < 1e-7 && (p.z - 7.0).abs() < 1e-7
            })
        })
        .unwrap();
    // A stored rail claims a sub-roundoff budget for its coordinates.
    // Its opposite rail's normal tolerance must not mask this bad input.
    edge::set_tolerance(&mut b, tight, f64::EPSILON, false, false).unwrap();
    let before = mass(&k, &mut b);
    let topology = |b: &Brep| {
        b.edge_list()
            .iter()
            .map(|&e| {
                (
                    e,
                    edge::tolerance(b, e).unwrap(),
                    edge::start_end_vertices(b, e).unwrap(),
                )
            })
            .collect::<Vec<_>>()
    };
    let edges = topology(&b);
    let err = delete_fillet_faces(&mut k, &mut b, &patches, None).unwrap_err();
    assert!(
        matches!(
            err.failure().map(|f| &f.reason),
            Some(Reason::Blend(BlendFailure::RemovalRefused(
                RemovalRefusal::RailTrimOutsideTolerance
            )))
        ),
        "{err:?}"
    );
    assert_eq!(topology(&b), edges);
    assert_eq!(mass(&k, &mut b), before);
}

#[test]
fn removes_a_concave_split_round_from_a_ball_on_a_plate() {
    let mut k = Kernel::default();
    let plate = create_cylinder(&mut k, Point3::new(0.0, 0.0, -2.0), 8.0, 2.0).unwrap();
    let ball = create_sphere(&mut k, Point3::new(0.0, 0.0, 1.0), 3.0).unwrap();
    let b = boolean_union(&mut k, plate, ball).unwrap();
    removes_split_ring(
        &mut k,
        b,
        8.0_f64.sqrt(),
        0.0,
        [464.0 * PI / 3.0, 176.0 * PI],
    );
}

#[test]
fn removes_the_round_between_a_post_and_a_turned_spherical_cap() {
    let mut k = Kernel::default();
    let post = create_cylinder(&mut k, Point3::ZERO, 4.0, 6.0).unwrap();
    let dome = spherical_cap(&mut k, 3.0, 6.0);
    let dome = turned(&mut k, dome, 1.5);
    let b = boolean_union(&mut k, post, dome).unwrap();
    removes_split_ring(&mut k, b, 4.0, 6.0, [340.0 * PI / 3.0, 84.0 * PI]);
}

#[test]
fn removes_the_round_between_two_turned_frustums() {
    let mut k = Kernel::default();
    let base = create_cone(&mut k, Point3::ZERO, 3.0, 5.0, 4.0).unwrap();
    let upper = create_cone(&mut k, Point3::new(0.0, 0.0, 4.0), 5.0, 2.0, 4.0).unwrap();
    let upper = turned(&mut k, upper, 2.0);
    let b = boolean_union(&mut k, base, upper).unwrap();
    removes_split_ring(
        &mut k,
        b,
        5.0,
        4.0,
        [352.0 * PI / 3.0, (48.0 + 16.0 * 5.0_f64.sqrt()) * PI],
    );
}

#[test]
fn removes_the_round_between_a_post_and_a_turned_frustum() {
    let mut k = Kernel::default();
    let post = create_cylinder(&mut k, Point3::ZERO, 5.0, 6.0).unwrap();
    let cone = create_cone(&mut k, Point3::new(0.0, 0.0, 6.0), 5.0, 2.0, 4.0).unwrap();
    let cone = turned(&mut k, cone, 1.0);
    let b = boolean_union(&mut k, post, cone).unwrap();
    removes_split_ring(&mut k, b, 5.0, 6.0, [202.0 * PI, 124.0 * PI]);
}

fn rounded_block(k: &mut Kernel) -> (Brep, Vec<FaceId>) {
    let mut block = create_box(k, Point3::ZERO, 20.0, 12.0, 8.0).unwrap();
    round_edges(k, &mut block, 3.0, |p| (p[0].z - p[2].z).abs() > 7.9);
    assert_eq!(block.face_list().len(), 10);
    round_edges(k, &mut block, 1.0, |p| {
        p.iter().all(|p| (p.z - 8.0).abs() < 1e-7)
    });
    let rim = round_faces(&block, 8.0);
    assert_eq!(rim.len(), 8);
    (block, rim)
}

#[test]
fn removes_the_eight_top_rim_rounds_and_keeps_all_four_upright_rounds() {
    let mut k = Kernel::default();
    let (mut block, mut rim) = rounded_block(&mut k);
    rim.swap(1, 4);
    let d = 3.0 / 2.0_f64.sqrt();
    // Both the planes and the four upright corner cylinders must reach z = 8 again.
    let xy = [
        (10.0, 0.0),
        (20.0, 6.0),
        (10.0, 12.0),
        (0.0, 6.0),
        (3.0 - d, 3.0 - d),
        (17.0 + d, 3.0 - d),
        (17.0 + d, 9.0 + d),
        (3.0 - d, 9.0 + d),
    ];
    let crest = xy.map(|(x, y)| Point3::new(x, y, 8.0));
    removes_chain(
        &mut k,
        &mut block,
        &rim,
        &crest,
        [8.0 * (204.0 + 9.0 * PI), 728.0 + 66.0 * PI],
    );
}

#[test]
fn removes_an_open_chain_between_two_ends_on_a_section_plane() {
    let mut k = Kernel::default();
    let (rounded, _) = rounded_block(&mut k);
    // Keep y <= 6. The section cuts through both long-side rounds, leaving
    // five faces between the two ends of the rim, all on the same planar cap.
    let half = create_box(&mut k, Point3::new(-1.0, -1.0, -1.0), 22.0, 7.0, 10.0).unwrap();
    let mut cut = boolean_intersection(&mut k, rounded, half).unwrap();
    let rim = round_faces(&cut, 8.0);
    assert_eq!(rim.len(), 5);
    let d = 3.0 / 2.0_f64.sqrt();
    let xy = [
        (0.0, 4.5),
        (3.0 - d, 3.0 - d),
        (10.0, 0.0),
        (17.0 + d, 3.0 - d),
        (20.0, 4.5),
    ];
    let crest = xy.map(|(x, y)| Point3::new(x, y, 8.0));
    // Half the uncut sharp volume and half its surface area, plus the 20 x 8 cap.
    removes_chain(
        &mut k,
        &mut cut,
        &rim,
        &crest,
        [4.0 * (204.0 + 9.0 * PI), 524.0 + 33.0 * PI],
    );
    for x in [0.0, 20.0] {
        assert_eq!(faces_at(&k, &mut cut, Point3::new(x, 6.0, 8.0)), 3);
    }
}

#[test]
fn incomplete_and_repeated_selections_leave_the_rounded_block_intact() {
    let mut k = Kernel::default();
    let (mut block, rim) = rounded_block(&mut k);
    block.set_editing_enabled(true);
    let before = mass(&k, &mut block);
    let vertices: Vec<_> = block
        .vertices()
        .ids()
        .map(|v| vertex::point(&block, v).unwrap())
        .collect();
    let mut repeated = rim.clone();
    repeated.push(rim[0]);
    for selection in [&[][..], &rim[..7], &repeated] {
        assert!(delete_fillet_faces(&mut k, &mut block, selection, None).is_err());
        assert_eq!(block.face_list().len(), 18);
        assert_eq!(mass(&k, &mut block), before);
        assert_eq!(
            block
                .vertices()
                .ids()
                .map(|v| vertex::point(&block, v).unwrap())
                .collect::<Vec<_>>(),
            vertices
        );
        assert!(check_brep(&mut k, &mut block, CheckLevel::Level2)
            .unwrap()
            .is_valid());
    }
}

#[test]
fn a_valid_tolerance_on_one_round_cannot_mask_an_invalid_neighbour() {
    let mut k = Kernel::default();
    let (mut block, rim) = rounded_block(&mut k);
    block.set_editing_enabled(true);
    let original_faces = block.face_list().to_vec();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1e-5] {
        solidcore::topology::face::p3::set_tolerance(&mut block, rim[0], bad, false, false)
            .unwrap();
        let err = delete_fillet_faces(&mut k, &mut block, &rim, None).unwrap_err();
        assert_eq!(err.kind(), solidcore::error::ErrorKind::InvalidInput);
        assert_eq!(block.face_list(), original_faces.as_slice());
    }
}

#[test]
fn observers_see_only_committed_fillet_removals() {
    use solidcore::error::EntityRef;
    use solidcore::kernel::CancelToken;
    use solidcore::topology::brep::p3::{notify_serial, topology_ref};
    use solidcore::topology::topology::TrackTopologyChanges;
    use solidcore::{BrepId, SolidError};

    // Measure the last poll of this edit, so cancellation is inside final validation,
    // after the Euler edits, without relying on a fixed count of solver iterations.
    let mut budget = u64::MAX;
    for commit in [true, false] {
        let mut k = Kernel::default();
        let (mut block, rim) = rounded_block(&mut k);
        block.set_editing_enabled(true);
        let before = block.face_list().to_vec();
        let serial = notify_serial(&mut k.context, &mut block).unwrap();
        let tracker =
            TrackTopologyChanges::new(Some(BrepId::from_index(0)), 1, true, Some(&mut k.context));
        let token = CancelToken::after_polls(budget);
        k.set_cancel_token(token.clone());
        let result = delete_fillet_faces(&mut k, &mut block, &rim, None);
        assert!(tracker.is_tracking(Some(&k.context)));
        if commit {
            result.unwrap();
            let removed = tracker.rm_list();
            for f in rim {
                assert!(removed.contains(&topology_ref(serial, EntityRef::Face(f)).key));
                assert!(!block.face_list().contains(&f));
            }
            budget = token.polls().unwrap();
        } else {
            assert!(
                matches!(result, Err(SolidError::Cancelled { .. })),
                "{result:?}"
            );
            assert_eq!(block.face_list(), before);
            assert!(
                tracker.lists().notify_events.is_empty(),
                "observer saw a refused edit"
            );
        }
    }
}
