// SPDX-License-Identifier: Apache-2.0
use super::{entries_lie_on_no_face, hole_support_faces};
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCylinder, AsmHistoricalEntityDelta,
    AsmHistoricalPlane, AsmHistoricalTopology, AsmHistoricalTopologyDelta, AsmHistoricalTransition,
};
use crate::records::feature::hole::DesignHoleConstruction;
use cadmpeg_ir::math::{Point3, Vector3};
use serde_json::{json, Value};

const TOP: i64 = 10;
const BOTTOM: i64 = 20;
const TOP_MM: [f64; 3] = [10.0, 15.0, 20.0];
const OTHER_MM: [f64; 3] = [30.0, 15.0, 20.0];
const TOP_CM: [f64; 3] = [1.0, 1.5, 2.0];
const OTHER_CM: [f64; 3] = [3.0, 1.5, 2.0];
const UP: [f64; 3] = [0.0, 0.0, 1.0];

/// A sketch-point carrier with one point record per entry point.
fn sketch_carrier(entries_cm: &[[f64; 3]], direction: [f64; 3]) -> DesignHoleConstruction {
    let locations: Vec<Value> = entries_cm
        .iter()
        .enumerate()
        .map(|(index, position)| {
            json!({
                "record_index": index, "position": position,
                "position_offset": index, "reference_type": 10
            })
        })
        .collect();
    serde_json::from_value(json!({
        "point_record_index": 1, "point_record_byte_offset": 2,
        "position": [0.0, 0.0, 0.0], "position_offset": 3,
        "direction": direction, "direction_offset": 4,
        "point_parameters": [0.0, 0.0], "point_parameter_offsets": [5, 6],
        "reference_type": 13, "reference_type_offset": 7,
        "input_record_indices": [8], "input_record_offsets": [9],
        "locations": locations
    }))
    .unwrap()
}

/// The preceding state: `(face, plane origin, plane normal)` per face.
fn preceding(faces: &[(i64, [f64; 3], [f64; 3])]) -> AsmHistoricalTopology {
    let point = |p: [f64; 3]| Point3::new(p[0], p[1], p[2]);
    AsmHistoricalTopology {
        faces: faces.iter().map(|face| face.0).collect(),
        surface_planes: faces
            .iter()
            .map(|(face, origin, normal)| AsmHistoricalPlane {
                surface: face + 100,
                origin: point(*origin),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
            })
            .collect(),
        face_surfaces: faces
            .iter()
            .map(|(face, _, _)| AsmHistoricalCarrierBinding {
                entity: *face,
                carrier: face + 100,
            })
            .collect(),
        ..AsmHistoricalTopology::default()
    }
}

/// One transition: inserted cylinders by axis point, updated faces, and rims.
struct Drill {
    cylinders: Vec<([f64; 3], [f64; 3])>,
    updated: Vec<i64>,
    rims: Vec<(i64, [f64; 3], [f64; 3])>,
}

impl Drill {
    /// One coaxial cylinder and one rim on `face` for each entry point.
    fn on(face: i64, entries_mm: &[[f64; 3]]) -> Self {
        Self {
            cylinders: entries_mm.iter().map(|entry| (*entry, UP)).collect(),
            updated: vec![face],
            rims: entries_mm.iter().map(|entry| (face, *entry, UP)).collect(),
        }
    }

    fn transition(&self) -> AsmHistoricalTransition {
        let inserted: Vec<i64> = (0i64..)
            .take(self.cylinders.len())
            .map(|index| 1000 + index)
            .collect();
        let cylinders: Vec<Value> = self
            .cylinders
            .iter()
            .zip(&inserted)
            .map(|((origin, axis), face)| {
                json!({
                    "surface": face + 100,
                    "origin": {"x": origin[0], "y": origin[1], "z": origin[2]},
                    "axis": {"x": axis[0], "y": axis[1], "z": axis[2]},
                    "radius": 2.5
                })
            })
            .collect();
        let bindings: Vec<Value> = inserted
            .iter()
            .map(|face| json!({"entity": face, "carrier": face + 100}))
            .collect();
        let mut face_loops = Vec::new();
        let mut loop_coedges = Vec::new();
        let mut coedges = Vec::new();
        let mut edge_curves = Vec::new();
        let mut curve_axes = Vec::new();
        for (index, (face, center, axis)) in self.rims.iter().enumerate() {
            let id = 2000 + 10 * i64::try_from(index).unwrap();
            face_loops.push(json!({"owner_ref": face, "member_refs": [id]}));
            loop_coedges.push(json!({"owner_ref": id, "member_refs": [id + 1]}));
            coedges.push(json!({
                "coedge": id + 1, "owner_loop": id, "edge": id + 2,
                "next": id + 1, "previous": id + 1, "radial_next": id + 1
            }));
            edge_curves.push(json!({"entity": id + 2, "carrier": id + 3}));
            curve_axes.push(json!({
                "curve": id + 3,
                "origin": {"x": center[0], "y": center[1], "z": center[2]},
                "direction": {"x": axis[0], "y": axis[1], "z": axis[2]}
            }));
        }
        AsmHistoricalTransition {
            previous_state_id: Some(1),
            records: AsmHistoricalEntityDelta::default(),
            topology: AsmHistoricalTopologyDelta {
                faces: AsmHistoricalEntityDelta {
                    inserted,
                    updated: self.updated.clone(),
                    ..AsmHistoricalEntityDelta::default()
                },
                ..AsmHistoricalTopologyDelta::default()
            },
            record_revisions: Vec::new(),
            rows: serde_json::from_value(json!({
                "surface_cylinders": {"set": cylinders},
                "face_surfaces": {"set": bindings},
                "face_loops": {"set": face_loops},
                "loop_coedges": {"set": loop_coedges},
                "coedge_topology": {"set": coedges},
                "edge_curves": {"set": edge_curves},
                "curve_axes": {"set": curve_axes}
            }))
            .unwrap(),
        }
    }
}

fn plate() -> AsmHistoricalTopology {
    preceding(&[(TOP, TOP_MM, UP), (BOTTOM, [10.0, 15.0, 0.0], UP)])
}

#[test]
fn support_is_the_updated_plane_through_the_entry_point_with_its_rim() {
    assert_eq!(
        hole_support_faces(
            &sketch_carrier(&[TOP_CM], UP),
            &Drill::on(TOP, &[TOP_MM]).transition(),
            &plate()
        ),
        Some(vec![TOP])
    );
}

#[test]
fn support_entry_points_convert_from_source_centimetres() {
    assert_eq!(
        hole_support_faces(
            &sketch_carrier(&[TOP_MM], UP),
            &Drill::on(TOP, &[TOP_MM]).transition(),
            &plate()
        ),
        None
    );
}

#[test]
fn support_plane_normal_of_either_sign_along_the_axis_qualifies() {
    let reversed = preceding(&[(TOP, TOP_MM, [0.0, 0.0, -1.0])]);
    assert_eq!(
        hole_support_faces(
            &sketch_carrier(&[TOP_CM], UP),
            &Drill::on(TOP, &[TOP_MM]).transition(),
            &reversed
        ),
        Some(vec![TOP])
    );
}

#[test]
fn support_of_several_entry_points_is_each_distinct_face_sorted() {
    let two_entries = sketch_carrier(&[TOP_CM, OTHER_CM], UP);
    assert_eq!(
        hole_support_faces(
            &two_entries,
            &Drill::on(TOP, &[TOP_MM, OTHER_MM]).transition(),
            &plate()
        ),
        Some(vec![TOP])
    );
    let pads = preceding(&[(40, TOP_MM, UP), (30, OTHER_MM, UP)]);
    let mut drill = Drill::on(40, &[TOP_MM, OTHER_MM]);
    drill.updated = vec![40, 30];
    drill.rims = vec![(40, TOP_MM, UP), (30, OTHER_MM, UP)];
    assert_eq!(
        hole_support_faces(&two_entries, &drill.transition(), &pads),
        Some(vec![30, 40])
    );
}

#[test]
fn support_needs_a_rim_on_the_face_for_every_entry_point() {
    let two_entries = sketch_carrier(&[TOP_CM, OTHER_CM], UP);
    for present in [TOP_MM, OTHER_MM] {
        let mut drill = Drill::on(TOP, &[TOP_MM, OTHER_MM]);
        drill.rims = vec![(TOP, present, UP)];
        assert_eq!(
            hole_support_faces(&two_entries, &drill.transition(), &plate()),
            None
        );
    }
}

#[test]
fn support_needs_a_plane_through_every_entry_point() {
    let above = [30.0, 15.0, 30.0];
    for entries in [[TOP_CM, [3.0, 1.5, 3.0]], [[3.0, 1.5, 3.0], TOP_CM]] {
        let mut drill = Drill::on(TOP, &[TOP_MM, above]);
        drill.cylinders = vec![(TOP_MM, UP), (above, UP)];
        assert_eq!(
            hole_support_faces(&sketch_carrier(&entries, UP), &drill.transition(), &plate()),
            None
        );
    }
}

#[test]
fn support_rim_must_be_centred_on_the_entry_point_along_the_axis() {
    let off_centre = Drill {
        rims: vec![(TOP, OTHER_MM, UP)],
        ..Drill::on(TOP, &[TOP_MM])
    };
    let tilted = Drill {
        rims: vec![(TOP, TOP_MM, [1.0, 0.0, 0.0])],
        ..Drill::on(TOP, &[TOP_MM])
    };
    let on_other_face = Drill {
        rims: vec![(BOTTOM, TOP_MM, UP)],
        ..Drill::on(TOP, &[TOP_MM])
    };
    for drill in [off_centre, tilted, on_other_face] {
        assert_eq!(
            hole_support_faces(
                &sketch_carrier(&[TOP_CM], UP),
                &drill.transition(),
                &plate()
            ),
            None
        );
    }
}

#[test]
fn support_needs_a_coaxial_inserted_cylinder_for_every_entry_point() {
    let two_entries = sketch_carrier(&[TOP_CM, OTHER_CM], UP);
    let mut drill = Drill::on(TOP, &[TOP_MM, OTHER_MM]);
    drill.cylinders = vec![(TOP_MM, UP)];
    assert_eq!(
        hole_support_faces(&two_entries, &drill.transition(), &plate()),
        None
    );
}

#[test]
fn support_refuses_an_inserted_cylinder_off_every_entry_axis() {
    let mut drill = Drill::on(TOP, &[TOP_MM]);
    drill.cylinders.push((OTHER_MM, UP));
    assert_eq!(
        hole_support_faces(
            &sketch_carrier(&[TOP_CM], UP),
            &drill.transition(),
            &plate()
        ),
        None
    );
}

#[test]
fn support_ignores_a_cylinder_row_of_a_face_that_is_not_inserted() {
    let mut transition = Drill::on(TOP, &[TOP_MM]).transition();
    transition.rows.surface_cylinders.set.push(
        serde_json::from_value(json!({
            "surface": 9999,
            "origin": {"x": 30.0, "y": 15.0, "z": 20.0},
            "axis": {"x": 0.0, "y": 0.0, "z": 1.0},
            "radius": 2.5
        }))
        .unwrap(),
    );
    assert_eq!(
        hole_support_faces(&sketch_carrier(&[TOP_CM], UP), &transition, &plate()),
        Some(vec![TOP])
    );
}

#[test]
fn support_needs_a_face_that_the_transition_updates() {
    let drill = Drill {
        updated: vec![BOTTOM],
        ..Drill::on(TOP, &[TOP_MM])
    };
    assert_eq!(
        hole_support_faces(
            &sketch_carrier(&[TOP_CM], UP),
            &drill.transition(),
            &plate()
        ),
        None
    );
}

#[test]
fn support_refuses_two_faces_that_both_carry_the_entry_point() {
    let coplanar = preceding(&[(TOP, TOP_MM, UP), (40, TOP_MM, UP)]);
    let drill = Drill {
        updated: vec![TOP, 40],
        rims: vec![(TOP, TOP_MM, UP), (40, TOP_MM, UP)],
        ..Drill::on(TOP, &[TOP_MM])
    };
    assert_eq!(
        hole_support_faces(
            &sketch_carrier(&[TOP_CM], UP),
            &drill.transition(),
            &coplanar
        ),
        None
    );
}

#[test]
fn support_plane_must_be_normal_to_the_axis() {
    let tilted = preceding(&[(TOP, TOP_MM, [1.0, 0.0, 0.0])]);
    assert_eq!(
        hole_support_faces(
            &sketch_carrier(&[TOP_CM], UP),
            &Drill::on(TOP, &[TOP_MM]).transition(),
            &tilted
        ),
        None
    );
}

#[test]
fn support_refuses_a_face_with_two_carrier_bindings() {
    let mut topology = plate();
    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: TOP,
        carrier: 777,
    });
    assert_eq!(
        hole_support_faces(
            &sketch_carrier(&[TOP_CM], UP),
            &Drill::on(TOP, &[TOP_MM]).transition(),
            &topology
        ),
        None
    );
}

#[test]
fn support_refuses_a_face_absent_from_the_preceding_state() {
    let mut topology = plate();
    topology.faces.retain(|face| *face != TOP);
    assert_eq!(
        hole_support_faces(
            &sketch_carrier(&[TOP_CM], UP),
            &Drill::on(TOP, &[TOP_MM]).transition(),
            &topology
        ),
        None
    );
}

#[test]
fn support_refuses_a_face_with_two_plane_rows() {
    let mut topology = plate();
    let duplicate = topology.surface_planes[0].clone();
    topology.surface_planes.push(duplicate);
    assert_eq!(
        hole_support_faces(
            &sketch_carrier(&[TOP_CM], UP),
            &Drill::on(TOP, &[TOP_MM]).transition(),
            &topology
        ),
        None
    );
}

#[test]
fn support_needs_an_admitted_placement() {
    let mut other_form = sketch_carrier(&[TOP_CM], UP);
    other_form.reference_type = 14;
    assert_eq!(
        hole_support_faces(
            &other_form,
            &Drill::on(TOP, &[TOP_MM]).transition(),
            &plate()
        ),
        None
    );
}

/// A 40 by 30 by 20 millimetre box with one corner at the origin.
fn box_faces() -> AsmHistoricalTopology {
    preceding(&[
        (1, [0.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
        (2, [0.0, 0.0, 20.0], UP),
        (3, [0.0, 0.0, 0.0], [-1.0, 0.0, 0.0]),
        (4, [40.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        (5, [0.0, 0.0, 0.0], [0.0, -1.0, 0.0]),
        (6, [0.0, 30.0, 0.0], [0.0, 1.0, 0.0]),
    ])
}

/// The box with face 7 on a cylinder of radius 5 mm about the z axis
/// through (20, 15).
fn box_with_boss() -> AsmHistoricalTopology {
    let mut topology = box_faces();
    topology.faces.push(7);
    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 7,
        carrier: 107,
    });
    topology.surface_cylinders.push(AsmHistoricalCylinder {
        surface: 107,
        origin: Point3::new(20.0, 15.0, -3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        radius: 5.0,
    });
    topology
}

#[test]
fn an_entry_point_inside_the_input_solid_lies_on_no_face() {
    let inside = sketch_carrier(&[[1.0, 1.5, 1.0], [3.0, 1.5, 1.0]], UP);
    assert!(entries_lie_on_no_face(&inside, &box_faces()));
    assert!(entries_lie_on_no_face(&inside, &box_with_boss()));
}

#[test]
fn an_entry_point_on_any_face_carrier_can_lie_on_that_face() {
    for entry in [
        [1.0, 1.5, 2.0],
        [9.0, 1.5, 2.0],
        [1.0, 1.5, 0.0],
        [4.0, 1.5, 1.0],
    ] {
        let carrier = sketch_carrier(&[[3.0, 1.5, 1.0], entry], UP);
        assert!(!entries_lie_on_no_face(&carrier, &box_faces()), "{entry:?}");
    }
    let on_boss = sketch_carrier(&[[2.5, 1.5, 1.0]], UP);
    assert!(entries_lie_on_no_face(&on_boss, &box_faces()));
    assert!(!entries_lie_on_no_face(&on_boss, &box_with_boss()));
}

#[test]
fn a_face_without_one_plane_or_cylinder_carrier_leaves_the_face_open() {
    let inside = sketch_carrier(&[[1.0, 1.5, 1.0]], UP);
    let mut unbound = box_faces();
    unbound.face_surfaces.retain(|binding| binding.entity != 2);
    assert!(!entries_lie_on_no_face(&inside, &unbound));
    let mut twice_bound = box_faces();
    twice_bound.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 2,
        carrier: 101,
    });
    assert!(!entries_lie_on_no_face(&inside, &twice_bound));
    let mut other_surface = box_faces();
    other_surface
        .surface_planes
        .retain(|plane| plane.surface != 102);
    assert!(!entries_lie_on_no_face(&inside, &other_surface));
    let mut plane_and_cylinder = box_with_boss();
    plane_and_cylinder.surface_planes.push(AsmHistoricalPlane {
        surface: 107,
        origin: Point3::new(0.0, 0.0, 50.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
    });
    assert!(!entries_lie_on_no_face(&inside, &plane_and_cylinder));
    let mut degenerate = box_faces();
    degenerate.surface_planes[1].normal = Vector3::new(0.0, 0.0, 0.0);
    assert!(!entries_lie_on_no_face(&inside, &degenerate));
}

#[test]
fn entries_lie_on_no_face_needs_entry_points_and_input_faces() {
    let inside = sketch_carrier(&[[1.0, 1.5, 1.0]], UP);
    assert!(!entries_lie_on_no_face(
        &inside,
        &AsmHistoricalTopology::default()
    ));
    let mut other_form = inside.clone();
    other_form.reference_type = 14;
    assert!(!entries_lie_on_no_face(&other_form, &box_faces()));
}
