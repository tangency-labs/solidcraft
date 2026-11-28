//! A 60 x 40 x 10 plate with an r10 boss on it and an r2 round at the boss's base has
//! its boss wall moved out by 1 (`SmBrep::LocalOperation`, the capi's
//! `smlib_offset_faces`). Only the wall gets a new surface, so the tangent edge between
//! it and the round is dropped across the 1 mm gap between them. Upstream ignores that
//! ("will be fixed later") and returns a body with an edge as loose as the gap; here the
//! local operation is refused. A split block's top piece turned and raised off its
//! coplanar twin reaches the refusal of the loose edge itself
//! (`OffsetFailure::LooseTangentEdge`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers
use solidcore::math::det;

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::Notifications;
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::boolean::api_brep::boolean_union;
use solidcore::error::SM_ERR;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_cylinder_surface};
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::local_operation::local_operation;
use solidcore::topology::FaceId;
use solidcore::{Brep, Kernel};

const AXIS: (f64, f64) = (30.0, 20.0);

fn axis_distance(p: Point3) -> f64 {
    det::hypot(p.x - AXIS.0, p.y - AXIS.1)
}

/// The plate with its boss and the r2 round at the boss's base.
fn part(k: &mut Kernel) -> Brep {
    let plate = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 10.0).unwrap();
    let boss = create_cylinder(k, Point3::new(AXIS.0, AXIS.1, 5.0), 10.0, 15.0).unwrap();
    let mut b = boolean_union(k, plate, boss).unwrap();
    let base: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let p = c.evaluate_point(k, e.interval.mid()).unwrap();
            (p.z - 10.0).abs() < 1e-9 && (axis_distance(p) - 10.0).abs() < 1e-9
        })
        .map(|(id, _)| id)
        .collect();
    assert!(!base.is_empty(), "the boss's base edge");
    fillet_edges(k, &mut b, &base, 2.0, 1, 1, 1.0).unwrap();
    b.refresh_cached_topology();
    b
}

/// The faces of the boss's wall: on the r10 circle about the boss's axis, above the round.
fn wall(k: &Kernel, b: &Brep) -> Vec<FaceId> {
    b.face_list()
        .iter()
        .copied()
        .filter(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            let p = s.evaluate_point(k, face.uv_domain.mid()).unwrap();
            p.z > 12.5 && (axis_distance(p) - 10.0).abs() < 1e-6
        })
        .collect()
}

/// The boss's wall given the r11 cylinder: the wall's faces, and the operation's result.
fn move_wall() -> (Vec<FaceId>, solidcore::Result<()>) {
    let mut k = Kernel::default();
    let mut b = part(&mut k);
    let faces = wall(&k, &b);
    assert!(!faces.is_empty(), "the boss's wall");
    let surfaces = faces
        .iter()
        .map(|_| {
            Surface::from(
                create_cylinder_surface(&k, Point3::new(AXIS.0, AXIS.1, 5.0), 11.0, 15.0).unwrap(),
            )
        })
        .collect();
    let mut notifications = Notifications::new(&mut b);
    let result = local_operation(
        &mut k,
        &mut b,
        &faces,
        surfaces,
        &mut notifications.context(),
    );
    (faces, result)
}

/// Upstream returns the body with the edge as loose as the gap. The wall meets itself
/// across a seam, and its UV loops, copied onto the surface it takes, are in the r10
/// cylinder's parameters, which the r11 cylinder's domain does not carry:
/// the seam's two UV lines do not lift to one curve on it, so the seam would have no
/// edge. The local operation refuses that up front, naming the wall's face, before
/// anything is changed or reached.
#[test]
fn wr_530_moved_boss_wall_beside_its_round_is_refused_as_its_seam_opened() {
    use solidcore::error::offset::OffsetFailure;
    use solidcore::error::Reason;
    let (faces, result) = move_wall();
    let e = result.unwrap_err();
    assert!(
        matches!(
            e.failure().map(|f| &f.reason),
            Some(Reason::Offset(OffsetFailure::SeamOpened { face })) if faces.contains(face)
        ),
        "{e}"
    );
    assert_eq!(e.sm_status(), Some(SM_ERR));
}

/// A 60 x 40 x 20 block whose top is imprinted along x = 30 (the capi's `smlib_imprint`
/// of a box over x > 30, z > 10).
fn split_block(k: &mut Kernel) -> Brep {
    use solidcore::boolean::api_brep::boolean_with_options;
    use solidcore::boolean::merge::MergeOptions;
    use solidcore::geom::a_object::BooleanOperation;
    let block = create_box(k, Point3::new(0.0, 0.0, 0.0), 60.0, 40.0, 20.0).unwrap();
    let cutter = create_box(k, Point3::new(30.0, -1.0, 10.0), 60.0, 42.0, 20.0).unwrap();
    let mut b = boolean_with_options(
        k,
        block,
        cutter,
        BooleanOperation::Imprint,
        MergeOptions::default(),
    )
    .unwrap()
    .result;
    b.refresh_cached_topology();
    b
}

/// The split block's top piece x < 30 given its own plane turned by -0.02 rad about the
/// split line (x = 30, z = 20) and raised 1: the piece's new plane is 1 above its twin
/// along their tangent edge, and not parallel to it, so no step wall closes the gap
/// (their normals differ by 0.02) and the planes, falling away from the edge, never meet
/// over it. The edge, lifted onto the piece's plane and dropped onto its twin's, lands
/// about 1 away: the local operation is refused by name
/// ([`OffsetFailure::LooseTangentEdge`]), the input as it came in.
#[test]
fn wr_530_turned_and_raised_piece_beside_its_coplanar_twin_is_refused_as_loose() {
    use solidcore::error::offset::OffsetFailure;
    use solidcore::error::Reason;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::math::Vec3;
    let mut k = Kernel::default();
    let mut b = split_block(&mut k);
    let piece = b
        .face_list()
        .iter()
        .copied()
        .find(|&f| {
            let face = b.face(f).unwrap();
            let s = face.surface.as_ref().unwrap();
            let p = s.evaluate_point(&k, face.uv_domain.mid()).unwrap();
            (p.z - 20.0).abs() < 1e-9 && p.x < 30.0
        })
        .expect("the top piece x < 30");
    // the face's own plane turned and raised, so it keeps the face's parameterization
    let theta: f64 = -0.02;
    let (c, s) = (det::cos(theta), det::sin(theta));
    let pivot = Point3::new(30.0, 0.0, 20.0);
    let turned_pivot = Point3::new(
        c * pivot.x + s * pivot.z,
        pivot.y,
        -s * pivot.x + c * pivot.z,
    );
    let origin = pivot + Vec3::new(0.0, 0.0, 1.0) - turned_pivot;
    let place = Axis2Placement::new(origin, Vec3::new(c, 0.0, -s), Vec3::new(0.0, 1.0, 0.0));
    let mut surface = b.face(piece).unwrap().surface.clone().unwrap();
    surface.transform(&k, &place, None).unwrap();
    let mut notifications = Notifications::new(&mut b);
    let before = format!("{b:?}");
    let e = local_operation(
        &mut k,
        &mut b,
        &[piece],
        vec![surface],
        &mut notifications.context(),
    )
    .unwrap_err();
    drop(notifications);
    let Some(Reason::Offset(OffsetFailure::LooseTangentEdge {
        edge: Some(edge),
        max_drop_to_surf,
        meet_tol,
    })) = e.failure().map(|f| &f.reason)
    else {
        panic!("not the loose tangent edge, by name: {e}");
    };
    // the refusal carries the size it measured and the limit it was held to: the
    // piece turned 0.02 rad and raised 1 lands well off its twin's plane
    assert!(*meet_tol > 0.0 && *meet_tol < 1e-2, "{meet_tol}");
    assert!(
        *max_drop_to_surf > *meet_tol,
        "{max_drop_to_surf} {meet_tol}"
    );
    let says = e.to_string();
    assert!(says.contains(&format!("{max_drop_to_surf:e}")), "{says}");
    assert!(says.contains(&format!("{meet_tol:e}")), "{says}");
    // the refusal names the input's tangent edge between the piece and its twin, along
    // the split line
    let mut faces = solidcore::topology::edge::faces(&b, *edge, None).unwrap();
    faces.sort();
    assert_eq!(faces.len(), 2, "{faces:?}");
    assert!(faces.contains(&piece), "{faces:?}");
    for v in [
        solidcore::topology::edge::start_vertex(&b, *edge)
            .unwrap()
            .unwrap(),
        solidcore::topology::edge::end_vertex(&b, *edge)
            .unwrap()
            .unwrap(),
    ] {
        let p = b.vertex(v).unwrap().point;
        assert!(
            (p.x - 30.0).abs() < 1e-9 && (p.z - 20.0).abs() < 1e-9,
            "{p:?}"
        );
    }
    assert_eq!(
        e.failure().and_then(|f| f.reason.entity()),
        Some(solidcore::error::EntityRef::Edge(*edge))
    );
    assert_eq!(e.sm_status(), Some(SM_ERR));
    assert_eq!(format!("{b:?}"), before, "refused, the input as it came in");
}
