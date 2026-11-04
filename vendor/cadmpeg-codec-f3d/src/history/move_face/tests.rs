// SPDX-License-Identifier: Apache-2.0
use super::moved_face_selection;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCylinder, AsmHistoricalPlane, AsmHistoricalTopology,
};
use cadmpeg_ir::features::{FaceMotion, FeatureDirection3};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::Length;
use std::collections::BTreeSet;

const NORMAL_Z: Vector3 = Vector3 {
    x: 0.0,
    y: 0.0,
    z: 1.0,
};
const CYLINDER_ORIGIN: Point3 = Point3 {
    x: 4.0,
    y: 4.0,
    z: 0.0,
};

fn binding(entity: i64, carrier: i64) -> AsmHistoricalCarrierBinding {
    AsmHistoricalCarrierBinding { entity, carrier }
}

fn plane(surface: i64, z: f64) -> AsmHistoricalPlane {
    plane_at(surface, Point3::new(1.0, 2.0, z), NORMAL_Z)
}

fn plane_at(surface: i64, origin: Point3, normal: Vector3) -> AsmHistoricalPlane {
    AsmHistoricalPlane {
        surface,
        origin,
        normal,
    }
}

fn cylinder_at(surface: i64, origin: Point3, axis: Vector3, radius: f64) -> AsmHistoricalCylinder {
    AsmHistoricalCylinder {
        surface,
        origin,
        axis,
        radius,
    }
}

/// Faces 10 and 20 on planes at z = 0 and z = 5, and face 30 on a cylinder.
fn input() -> AsmHistoricalTopology {
    AsmHistoricalTopology {
        faces: vec![10, 20, 30],
        face_surfaces: vec![binding(10, 100), binding(20, 200), binding(30, 300)],
        surface_planes: vec![plane(100, 0.0), plane(200, 5.0)],
        surface_cylinders: vec![cylinder_at(300, CYLINDER_ORIGIN, NORMAL_Z, 2.0)],
        ..AsmHistoricalTopology::default()
    }
}

/// The input with new carriers, where face 10 moves to z = 2.
fn top_moves_up() -> AsmHistoricalTopology {
    let mut output = input();
    output.face_surfaces = vec![binding(10, 110), binding(20, 210), binding(30, 310)];
    output.surface_planes = vec![plane(110, 2.0), plane(210, 5.0)];
    output.surface_cylinders = vec![cylinder_at(310, CYLINDER_ORIGIN, NORMAL_Z, 2.0)];
    output
}

fn with_plane_10(origin: Point3, normal: Vector3) -> AsmHistoricalTopology {
    let mut output = top_moves_up();
    output.surface_planes[0] = plane_at(110, origin, normal);
    output
}

fn with_cylinder(row: AsmHistoricalCylinder) -> AsmHistoricalTopology {
    let mut output = top_moves_up();
    output.surface_cylinders[0] = row;
    output
}

fn offset(distance: f64) -> FaceMotion {
    FaceMotion::Offset {
        distance: Length::new(distance).unwrap(),
    }
}

fn translate(x: f64, y: f64, z: f64) -> FaceMotion {
    let shift = Vector3::new(x, y, z);
    FaceMotion::Translate {
        direction: FeatureDirection3::new(shift.unit().unwrap()).unwrap(),
        distance: Length::new(shift.norm()).unwrap(),
    }
}

fn select(
    members: &[u32],
    motion: &FaceMotion,
    updated: &[i64],
    output: &AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    let updated: BTreeSet<i64> = updated.iter().copied().collect();
    moved_face_selection(members, motion, &updated, &input(), output)
}

#[test]
fn the_face_whose_plane_follows_a_translation_is_the_selection() {
    assert_eq!(
        select(&[1], &translate(0.0, 0.0, 2.0), &[10, 20], &top_moves_up()),
        Some(vec![10])
    );
}

#[test]
fn the_face_whose_plane_follows_an_offset_of_either_sign_is_the_selection() {
    for distance in [2.0, -2.0] {
        assert_eq!(
            select(&[1], &offset(distance), &[10, 20], &top_moves_up()),
            Some(vec![10])
        );
    }
}

#[test]
fn a_translation_within_the_plane_moves_the_row_origin() {
    let output = with_plane_10(Point3::new(4.0, 2.0, 0.0), NORMAL_Z);
    assert_eq!(
        select(&[1], &translate(3.0, 0.0, 0.0), &[10], &output),
        Some(vec![10])
    );
}

#[test]
fn a_plane_row_that_moves_another_way_does_not_follow_the_motion() {
    for motion in [
        translate(0.0, 0.0, 3.0),
        translate(0.0, 2.0, 0.0),
        offset(3.0),
    ] {
        assert_eq!(select(&[1], &motion, &[10, 20], &top_moves_up()), None);
    }
}

#[test]
fn an_offset_shift_within_the_plane_does_not_follow_the_offset() {
    let output = with_plane_10(Point3::new(3.0, 2.0, 0.0), NORMAL_Z);
    assert_eq!(select(&[1], &offset(2.0), &[10], &output), None);
}

#[test]
fn a_turned_plane_does_not_follow_the_motion() {
    let output = with_plane_10(Point3::new(1.0, 2.0, 2.0), Vector3::new(0.0, 1.0, 0.0));
    assert_eq!(
        select(&[1], &translate(0.0, 0.0, 2.0), &[10], &output),
        None
    );
    assert_eq!(select(&[1], &offset(2.0), &[10], &output), None);
}

#[test]
fn a_cylinder_follows_a_translation_with_its_radius() {
    let moved = |radius| {
        with_cylinder(cylinder_at(
            310,
            Point3::new(5.0, 4.0, 0.0),
            NORMAL_Z,
            radius,
        ))
    };
    assert_eq!(
        select(&[1], &translate(1.0, 0.0, 0.0), &[30], &moved(2.0)),
        Some(vec![30])
    );
    assert_eq!(
        select(&[1], &translate(1.0, 0.0, 0.0), &[30], &moved(3.0)),
        None
    );
    assert_eq!(
        select(&[1], &translate(2.0, 0.0, 0.0), &[30], &moved(2.0)),
        None
    );
}

#[test]
fn a_turned_cylinder_does_not_follow_a_translation() {
    let output = with_cylinder(cylinder_at(
        310,
        Point3::new(5.0, 4.0, 0.0),
        Vector3::new(1.0, 0.0, 0.0),
        2.0,
    ));
    assert_eq!(
        select(&[1], &translate(1.0, 0.0, 0.0), &[30], &output),
        None
    );
}

#[test]
fn a_cylinder_follows_an_offset_on_its_axis_line() {
    let moved = |origin, radius| with_cylinder(cylinder_at(310, origin, NORMAL_Z, radius));
    let along_axis = Point3::new(4.0, 4.0, 9.0);
    for radius in [3.0, 1.0] {
        assert_eq!(
            select(&[1], &offset(1.0), &[30], &moved(along_axis, radius)),
            Some(vec![30])
        );
    }
    assert_eq!(
        select(&[1], &offset(2.0), &[30], &moved(along_axis, 3.0)),
        None
    );
    assert_eq!(
        select(
            &[1],
            &offset(1.0),
            &[30],
            &moved(Point3::new(5.0, 4.0, 0.0), 3.0)
        ),
        None
    );
}

#[test]
fn a_turned_cylinder_does_not_follow_an_offset() {
    let output = with_cylinder(cylinder_at(
        310,
        CYLINDER_ORIGIN,
        Vector3::new(1.0, 0.0, 0.0),
        3.0,
    ));
    assert_eq!(select(&[1], &offset(1.0), &[30], &output), None);
}

#[test]
fn every_moved_face_counts_against_the_members() {
    let mut both = top_moves_up();
    both.surface_planes[1] = plane(210, 7.0);
    let motion = translate(0.0, 0.0, 2.0);
    assert_eq!(
        select(&[1, 2], &motion, &[10, 20], &both),
        Some(vec![10, 20])
    );
    assert_eq!(select(&[1], &motion, &[10, 20], &both), None);
    assert_eq!(select(&[1, 2, 3], &motion, &[10, 20], &both), None);
}

#[test]
fn a_face_that_no_transition_updates_does_not_count() {
    let mut both = top_moves_up();
    both.surface_planes[1] = plane(210, 7.0);
    assert_eq!(
        select(&[1], &translate(0.0, 0.0, 2.0), &[10], &both),
        Some(vec![10])
    );
}

#[test]
fn a_repeated_member_counts_once() {
    assert_eq!(
        select(
            &[1, 1],
            &translate(0.0, 0.0, 2.0),
            &[10, 20],
            &top_moves_up()
        ),
        Some(vec![10])
    );
}

#[test]
fn an_updated_face_without_a_row_in_the_input_states_no_selection() {
    let mut without = input();
    without.surface_planes.retain(|row| row.surface != 200);
    let updated: BTreeSet<i64> = [10, 20].into_iter().collect();
    assert_eq!(
        moved_face_selection(
            &[1],
            &translate(0.0, 0.0, 2.0),
            &updated,
            &without,
            &top_moves_up()
        ),
        None
    );
}

#[test]
fn an_updated_face_without_a_row_in_the_output_states_no_selection() {
    let mut output = top_moves_up();
    output.surface_planes.retain(|row| row.surface != 210);
    assert_eq!(
        select(&[1], &translate(0.0, 0.0, 2.0), &[10, 20], &output),
        None
    );
}

#[test]
fn an_updated_face_outside_either_state_states_no_selection() {
    assert_eq!(
        select(&[1], &translate(0.0, 0.0, 2.0), &[10, 99], &top_moves_up()),
        None
    );
    let mut output = top_moves_up();
    output.face_surfaces.retain(|row| row.entity != 20);
    assert_eq!(
        select(&[1], &translate(0.0, 0.0, 2.0), &[10, 20], &output),
        None
    );
}

#[test]
fn a_slot_with_two_rows_states_no_selection() {
    let mut output = top_moves_up();
    output.surface_planes.push(plane(210, 6.0));
    assert_eq!(
        select(&[1], &translate(0.0, 0.0, 2.0), &[10, 20], &output),
        None
    );
    let mut output = top_moves_up();
    output.face_surfaces.push(binding(20, 110));
    assert_eq!(
        select(&[1], &translate(0.0, 0.0, 2.0), &[10, 20], &output),
        None
    );
}

#[test]
fn a_surface_with_a_plane_and_a_cylinder_row_states_no_selection() {
    let mut output = top_moves_up();
    output
        .surface_cylinders
        .push(cylinder_at(210, CYLINDER_ORIGIN, NORMAL_Z, 1.0));
    assert_eq!(
        select(&[1], &translate(0.0, 0.0, 2.0), &[10, 20], &output),
        None
    );
}

#[test]
fn no_moved_face_states_no_selection() {
    assert_eq!(
        select(&[], &translate(0.0, 0.0, 2.0), &[20], &top_moves_up()),
        None
    );
}

#[test]
fn a_zero_offset_states_no_selection() {
    assert_eq!(select(&[1], &offset(0.0), &[20], &top_moves_up()), None);
}

#[test]
fn a_rotation_states_no_selection() {
    let rotation = FaceMotion::Rotate {
        axis_origin: cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
        axis_dir: FeatureDirection3::Z_AXIS,
        angle: cadmpeg_ir::scalar::Angle::new(1.0).unwrap(),
    };
    assert_eq!(select(&[1], &rotation, &[10, 20], &top_moves_up()), None);
}

#[test]
fn a_plane_row_that_moves_against_its_normal_follows_an_offset() {
    let output = with_plane_10(Point3::new(1.0, 2.0, -2.0), NORMAL_Z);
    for distance in [2.0, -2.0] {
        assert_eq!(
            select(&[1], &offset(distance), &[10], &output),
            Some(vec![10])
        );
    }
}
