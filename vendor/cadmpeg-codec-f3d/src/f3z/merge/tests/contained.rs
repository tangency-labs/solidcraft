// SPDX-License-Identifier: Apache-2.0
//! External occurrences placed by their contained occurrences' component paths.

use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, PrototypeReference};
use cadmpeg_ir::transform::Transform;

use crate::loss::F3dLossCode;
use crate::records::feature::assembly_features::{
    DesignComponentOccurrence, DesignComponentOccurrenceDraft, DesignComponentOccurrencePlacement,
};
use crate::records::xref::{XrefContainedPose, XrefOwnedPose, XrefPlacementSource, XrefReference};
use crate::test_support::{local_reference, typed_cross_document_reference};

const ROLE: &str = "aaaabbbb-cccc-dddd-eeee-ffff00001111";
const ROOT_COMPONENT: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";
const PLACEMENT: &str = "CE2913AA-CFE0-4F04-9102-24424ED3BCFA";

fn translation(x: f64, y: f64, z: f64) -> [[f64; 4]; 4] {
    [
        [1.0, 0.0, 0.0, x],
        [0.0, 1.0, 0.0, y],
        [0.0, 0.0, 1.0, z],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// A component-path carrier: the referenced design's root component and its
/// placement `contained`, this document's placement 8506, then `owners`.
fn carrier(
    entity_id: u64,
    first_type: &str,
    second_role: &str,
    contained: u64,
    owners: &[u64],
    transform: [[f64; 4]; 4],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&u32::try_from(3 + owners.len()).unwrap().to_le_bytes());
    bytes.extend(typed_cross_document_reference(250, first_type, ROLE));
    bytes.extend_from_slice(&20_u32.to_le_bytes());
    bytes.extend(typed_cross_document_reference(
        contained,
        PLACEMENT,
        second_role,
    ));
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    for (discriminator, target) in std::iter::once(8506)
        .chain(owners.iter().copied())
        .enumerate()
    {
        bytes.extend(local_reference(target));
        bytes.extend_from_slice(&u32::try_from(discriminator).unwrap().to_le_bytes());
    }
    bytes.push(0);
    bytes.push(0);
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(8506));
    bytes.push(2);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

/// A component-path carrier of the external occurrence itself: the referenced
/// design's root component, this document's placement 8506, then `owners`.
fn owned_carrier(entity_id: u64, owners: &[u64], transform: [[f64; 4]; 4]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&u32::try_from(2 + owners.len()).unwrap().to_le_bytes());
    bytes.extend(typed_cross_document_reference(250, ROOT_COMPONENT, ROLE));
    bytes.extend_from_slice(&20_u32.to_le_bytes());
    for (discriminator, target) in std::iter::once(8506)
        .chain(owners.iter().copied())
        .enumerate()
    {
        bytes.extend(local_reference(target));
        bytes.extend_from_slice(&u32::try_from(discriminator).unwrap().to_le_bytes());
    }
    bytes.push(0);
    bytes.push(0);
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(local_reference(8506));
    bytes.push(2);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&42_u32.to_le_bytes());
    bytes.extend(local_reference(8));
    bytes.extend(local_reference(3));
    bytes.extend(local_reference(6));
    bytes
}

fn owned(entity_id: u64, owners: &[u64], world: [[f64; 4]; 4]) -> XrefOwnedPose {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let bytes = owned_carrier(entity_id, owners, world);
    let pose = crate::xref::owned_component_path_pose(&bytes, magic).unwrap();
    XrefOwnedPose {
        carrier_record_index: pose.carrier_record_index,
        frame_bytes: bytes.into(),
        owner_record_indices: pose.owner_record_indices,
        transform: pose.transform.map(|rows| rows.try_into().unwrap()),
    }
}

fn typed_owned(entity_id: u64, owners: &[u64], world: [[f64; 4]; 4]) -> XrefOwnedPose {
    let mut pose = owned(entity_id, owners, world);
    let mut bytes = pose.frame_bytes.to_vec();
    let mut at = 24;
    crate::bytes::take_reference(&bytes, &mut at).unwrap();
    at += 4;
    for target in std::iter::once(8506).chain(owners.iter().copied()) {
        bytes.splice(at..at + 11, inline_local(target, PLACEMENT));
        at += 55;
    }
    pose.frame_bytes = bytes.into();
    pose
}

fn reference(contained_poses: Vec<XrefContainedPose>) -> XrefReference {
    with_owned(contained_poses, Vec::new())
}

fn with_owned(
    contained_poses: Vec<XrefContainedPose>,
    owned_poses: Vec<XrefOwnedPose>,
) -> XrefReference {
    XrefReference {
        current_pose: None,
        operation_pose: None,
        contained_poses,
        owned_poses,
        id: "f3d:xref:reference#0-occurrence-0".to_owned(),
        ordinal: 0,
        occurrence_ordinal: 0,
        placement_source: Some(XrefPlacementSource {
            frame_bytes: Vec::new().into(),
            occurrence_guid: None,
            stream: "Design".to_owned(),
            carrier_record_index: 8506,
            owner: None,
        }),
        from: "root.f3d".to_owned().try_into().unwrap(),
        relative_path: "part.f3d".to_owned().try_into().unwrap(),
        neutron_role: ROLE.to_owned().try_into().unwrap(),
        neutron_data: String::new(),
        transform: None,
    }
}

fn retained(
    entity_id: u64,
    contained: u64,
    owners: &[u64],
    world: [[f64; 4]; 4],
) -> XrefContainedPose {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let bytes = carrier(entity_id, ROOT_COMPONENT, ROLE, contained, owners, world);
    let pose = crate::xref::contained_component_path_pose(&bytes, magic).unwrap();
    XrefContainedPose {
        carrier_record_index: pose.carrier_record_index,
        frame_bytes: bytes.into(),
        contained_record_index: pose.contained_record_index,
        owner_record_indices: pose.owner_record_indices,
        transform: pose.transform.map(|rows| rows.try_into().unwrap()),
    }
}

#[test]
fn a_contained_component_path_carrier_names_its_external_placement_and_owners() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let world = translation(20.0, 0.3, 5.0);
    let bytes = carrier(11, ROOT_COMPONENT, ROLE, 496, &[9222], world);
    let pose = crate::xref::contained_component_path_pose(&bytes, magic).unwrap();
    assert_eq!(
        (
            pose.carrier_record_index,
            pose.component_record_index,
            pose.contained_record_index,
            pose.placement_record_index,
            pose.owner_record_indices.as_slice(),
            pose.role.as_str(),
        ),
        (11, 250, 496, 8506, &[9222][..], ROLE)
    );
    assert_eq!(pose.transform, Some(world));
    // The first element names a root component and both carry one role.
    let wrong_type = carrier(11, PLACEMENT, ROLE, 496, &[9222], world);
    assert!(crate::xref::contained_component_path_pose(&wrong_type, magic).is_none());
    let other_role = carrier(11, ROOT_COMPONENT, "other", 496, &[9222], world);
    assert!(crate::xref::contained_component_path_pose(&other_role, magic).is_none());
    // A component-path carrier of an external occurrence itself is not one.
    assert!(crate::xref::component_path_pose(&bytes, magic).is_none());

    let reference = reference(vec![retained(11, 496, &[9222], world)]);
    assert!(crate::xref::contained_poses_match(&reference));
    let mut moved = reference.clone();
    moved.contained_poses[0].transform = Some(translation(21.0, 0.3, 5.0).try_into().unwrap());
    assert!(!crate::xref::contained_poses_match(&moved));
    let mut owners = reference.clone();
    owners.contained_poses[0].owner_record_indices = vec![1];
    assert!(!crate::xref::contained_poses_match(&owners));
    let mut placement = reference;
    placement
        .placement_source
        .as_mut()
        .unwrap()
        .carrier_record_index = 1;
    assert!(!crate::xref::contained_poses_match(&placement));
}

fn occurrence(id: &str, parent: Option<&str>, native_ref: Option<&str>, x: f64) -> Occurrence {
    Occurrence {
        id: OccurrenceId::mint(id).unwrap(),
        prototype: PrototypeReference::Unresolved {},
        parent: parent.map_or(OccurrenceParent::Root {}, |parent| {
            OccurrenceParent::Occurrence {
                occurrence: OccurrenceId::mint(parent).unwrap(),
            }
        }),
        ordinal: 0,
        transform: Transform::affine([
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap(),
        linked_prototype: None,
        scale: [cadmpeg_ir::scalar::FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: native_ref.map(str::to_owned),
    }
}

fn local_carrier(id: &str, record_index: u32) -> DesignComponentOccurrence {
    DesignComponentOccurrence::try_new(DesignComponentOccurrenceDraft {
        id: id.to_owned(),
        class_tag: crate::records::references::DesignClassTag::try_from("290".to_owned()).unwrap(),
        record_index,
        byte_offset: u64::from(record_index),
        component_record_index: 1,
        component_guid: "2ad641b3-9ea3-4e5f-a4da-926c3faeccae"
            .to_owned()
            .try_into()
            .unwrap(),
        occurrence_guid: crate::records::mesh::DesignRelaxedGuidText::try_from(format!(
            "00000000-0000-4000-8000-{record_index:012}"
        ))
        .unwrap()
        .into(),
        placement: DesignComponentOccurrencePlacement::Base,
        owner_entity: 1,
    })
    .unwrap()
}

/// An owner at x = 5 mm holding the external instance (stored at 10 mm), whose
/// member root holds parts 496 at x = 30 mm and 497 at x = -30 mm, with these
/// contained poses. Returns the instance's placement and the loss codes.
fn place(poses: Vec<XrefContainedPose>) -> (Transform, Vec<cadmpeg_ir::report::loss::LossKind>) {
    place_reference(reference(poses))
}

fn place_reference(
    reference: XrefReference,
) -> (Transform, Vec<cadmpeg_ir::report::loss::LossKind>) {
    let (transform, losses, _) = place_reference_stored_at(reference, 10.0);
    (transform, losses)
}

/// [`place_reference`] with the instance stored at `stored_x` millimetres, and
/// the number of instances that left the identity.
fn place_reference_stored_at(
    reference: XrefReference,
    stored_x: f64,
) -> (Transform, Vec<cadmpeg_ir::report::loss::LossKind>, usize) {
    let (ir, losses, count) = placed_reference(reference, stored_x);
    (ir.model.occurrences[3].transform, losses, count)
}

fn placed_reference(
    reference: XrefReference,
    stored_x: f64,
) -> (
    cadmpeg_ir::CadIr,
    Vec<cadmpeg_ir::report::loss::LossKind>,
    usize,
) {
    placed_reference_edit(reference, stored_x, |_| {})
}

fn placed_reference_edit(
    reference: XrefReference,
    stored_x: f64,
    prepare: impl FnOnce(&mut cadmpeg_ir::CadIr),
) -> (
    cadmpeg_ir::CadIr,
    Vec<cadmpeg_ir::report::loss::LossKind>,
    usize,
) {
    const OWNER: &str = "f3d:Design/BulkStream.dat:design-component-occurrence#9222";
    const OUTER: &str = "f3d:Design/BulkStream.dat:design-component-occurrence#9300";
    const PART: &str =
        "f3d:xref/role/occurrence-0/Design/BulkStream.dat:design-component-occurrence#";
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.occurrences = vec![
        occurrence("f3d:model:occurrence#root", None, None, 0.0),
        occurrence(
            "f3d:model:occurrence#outer",
            Some("f3d:model:occurrence#root"),
            Some(OUTER),
            0.0,
        ),
        occurrence(
            "f3d:model:occurrence#owner",
            Some("f3d:model:occurrence#outer"),
            Some(OWNER),
            5.0,
        ),
        occurrence(
            "f3d:model:occurrence#owner:xref-0-0",
            Some("f3d:model:occurrence#owner"),
            Some("f3d:xref:reference#0-occurrence-0"),
            stored_x,
        ),
        occurrence(
            "f3d:xref/role/occurrence-0/model:occurrence#member-root",
            Some("f3d:model:occurrence#owner:xref-0-0"),
            None,
            0.0,
        ),
        occurrence(
            "f3d:xref/role/occurrence-0/model:occurrence#part-496",
            Some("f3d:xref/role/occurrence-0/model:occurrence#member-root"),
            Some(&format!("{PART}496")),
            30.0,
        ),
        occurrence(
            "f3d:xref/role/occurrence-0/model:occurrence#part-497",
            Some("f3d:xref/role/occurrence-0/model:occurrence#member-root"),
            Some(&format!("{PART}497")),
            -30.0,
        ),
    ];
    prepare(&mut ir);
    let first_native = ir.model.occurrences[5].native_ref.clone().unwrap();
    crate::test_support::native_test::update_f3d_native(&mut ir, |native| {
        native.design_component_occurrences = vec![
            local_carrier(OWNER, 9222),
            local_carrier(OUTER, 9300),
            local_carrier(&first_native, 496),
            local_carrier(&format!("{PART}497"), 497),
        ];
        native.xref_references = vec![reference.clone()];
    });
    let table = crate::xref::XrefTable {
        references: vec![reference],
        ..Default::default()
    };
    let mut losses = Vec::new();
    let left_identity = super::super::place_contained_poses(
        &cadmpeg_test_support::service_decode_context(),
        &mut ir,
        &table,
        &mut losses,
    )
    .unwrap();
    (
        ir,
        losses.into_iter().map(|loss| loss.code).collect(),
        left_identity,
    )
}

#[test]
fn an_external_occurrence_takes_the_pose_its_contained_occurrences_give() {
    // The parts' world poses put the instance at x = 200 mm, y = 40 mm: below
    // the owner at 5 mm that is 195 mm, in place of its stored 10 mm.
    let poses = || {
        vec![
            retained(11, 496, &[9222], translation(23.0, 4.0, 0.0)),
            retained(12, 497, &[9222], translation(17.0, 4.0, 0.0)),
        ]
    };
    let (transform, losses) = place(poses());
    let rows = transform.rows();
    assert!(
        (rows[0][3] - 195.0).abs() < 1e-9 && (rows[1][3] - 40.0).abs() < 1e-9,
        "{rows:?}"
    );
    assert_eq!(losses, [F3dLossCode::XrefContainedPose.kind()]);

    // A carrier that lists fewer owners and agrees changes nothing; one that disagrees,
    // as when it keeps a pose from before an owner moved, yields to those that list more.
    for x in [23.0, 25.0] {
        let mut shorter = poses();
        shorter.push(retained(13, 496, &[], translation(x, 4.0, 0.0)));
        let (transform, losses) = place(shorter);
        assert!(
            (transform.rows()[0][3] - 195.0).abs() < 1e-9,
            "{:?}",
            transform.rows()
        );
        assert_eq!(losses, [F3dLossCode::XrefContainedPose.kind()]);
    }

    // Parts that give two poses leave the stored placement.
    let mut disagreeing = poses();
    disagreeing[1] = retained(12, 497, &[9222], translation(18.0, 4.0, 0.0));
    let (transform, losses) = place(disagreeing);
    assert!((transform.rows()[0][3] - 10.0).abs() < 1e-9);
    assert_eq!(losses, [F3dLossCode::XrefContainedPoseDisagrees.kind()]);

    // A carrier whose owner chain is not the instance's binds nothing.
    let (transform, losses) = place(vec![retained(
        11,
        496,
        &[1234],
        translation(23.0, 4.0, 0.0),
    )]);
    assert!((transform.rows()[0][3] - 10.0).abs() < 1e-9);
    assert_eq!(losses, [F3dLossCode::XrefContainedPoseNative.kind()]);
}

#[test]
fn an_owned_external_occurrence_takes_its_own_pose_unless_a_contained_pose_places_it() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let bytes = owned_carrier(11, &[9222], translation(20.0, 4.0, 0.0));
    let pose = crate::xref::owned_component_path_pose(&bytes, magic).unwrap();
    assert_eq!(
        (
            pose.placement_record_index,
            pose.owner_record_indices.as_slice(),
            pose.role.as_str()
        ),
        (8506, &[9222][..], ROLE)
    );
    // Its own carrier lists an owner; a two-element carrier is #486's form.
    assert!(crate::xref::owned_component_path_pose(
        &owned_carrier(11, &[], translation(20.0, 4.0, 0.0)),
        magic
    )
    .is_none());
    assert!(crate::xref::contained_component_path_pose(&bytes, magic).is_none());
    let reference = with_owned(
        Vec::new(),
        vec![owned(11, &[9222], translation(20.0, 4.0, 0.0))],
    );
    assert!(crate::xref::owned_poses_match(&reference));
    let mut moved = reference.clone();
    moved.owned_poses[0].transform = Some(translation(21.0, 4.0, 0.0).try_into().unwrap());
    assert!(!crate::xref::owned_poses_match(&moved));

    // The instance's own world pose, 200 mm, is 195 mm below the owner at 5 mm.
    let (transform, losses) = place_reference(reference);
    assert!(
        (transform.rows()[0][3] - 195.0).abs() < 1e-9,
        "{:?}",
        transform.rows()
    );
    assert_eq!(losses, [F3dLossCode::XrefOwnedPose.kind()]);

    // Own carriers that list the most owners and disagree leave the placement.
    let (transform, losses) = place_reference(with_owned(
        Vec::new(),
        vec![
            owned(11, &[9222], translation(20.0, 4.0, 0.0)),
            owned(12, &[9222], translation(21.0, 4.0, 0.0)),
        ],
    ));
    assert!(
        (transform.rows()[0][3] - 10.0).abs() < 1e-9,
        "{:?}",
        transform.rows()
    );
    assert_eq!(losses, [F3dLossCode::XrefOwnedPoseDisagrees.kind()]);

    // An own carrier that lists fewer owners yields to the one that lists more.
    let (transform, losses) = place_reference(with_owned(
        Vec::new(),
        vec![
            owned(12, &[9222], translation(21.0, 4.0, 0.0)),
            owned(11, &[9222, 9300], translation(20.0, 4.0, 0.0)),
        ],
    ));
    assert!(
        (transform.rows()[0][3] - 195.0).abs() < 1e-9,
        "{:?}",
        transform.rows()
    );
    assert_eq!(losses, [F3dLossCode::XrefOwnedPose.kind()]);

    // A matrix that is not a rigid motion is not an own carrier at all.
    let mut scaled = translation(20.0, 4.0, 0.0);
    scaled[0][0] = 2.0;
    assert!(
        crate::xref::owned_component_path_pose(&owned_carrier(13, &[9222], scaled), magic)
            .is_none()
    );

    // Independent carriers place the insertion and the contained instance.
    let (transform, losses) = place_reference(with_owned(
        vec![retained(12, 496, &[9222], translation(33.0, 4.0, 0.0))],
        vec![owned(11, &[9222], translation(20.0, 4.0, 0.0))],
    ));
    assert!(
        (transform.rows()[0][3] - 295.0).abs() < 1e-9,
        "{:?}",
        transform.rows()
    );
    assert_eq!(
        losses,
        [
            F3dLossCode::XrefContainedPose.kind(),
            F3dLossCode::XrefOwnedPoseNative.kind()
        ]
    );
}

const NESTED_ROLE: &str = "ccccdddd-eeee-ffff-0000-111122223333";

/// An inline-typed cross-document element, as the nested generation stores it.
fn inline_cross_document(target: u64, type_guid: &str, link_name: &str) -> Vec<u8> {
    use crate::test_support::{lp_ascii, lp_utf16};
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    lp_ascii(&mut bytes, type_guid);
    bytes.push(1);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    bytes.push(0);
    lp_utf16(&mut bytes, link_name);
    bytes.push(0);
    bytes
}

/// A same-document element that carries its inline type.
fn inline_local(target: u64, type_guid: &str) -> Vec<u8> {
    use crate::test_support::lp_ascii;
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    lp_ascii(&mut bytes, type_guid);
    bytes.extend_from_slice(&[0, 0]);
    bytes
}

/// A nested-design component-path carrier: the root component of the design
/// that the referenced design places, this document's placement 8506 and that
/// nested occurrence's placement `contained` in the referenced design. The
/// document placement follows the root component unless `placement_first`.
fn nested_carrier(
    entity_id: u64,
    placement_first: bool,
    contained: u64,
    transform: [[f64; 4]; 4],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"284");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    let root = inline_cross_document(152, ROOT_COMPONENT, NESTED_ROLE);
    let placement = inline_local(8506, PLACEMENT);
    let (first, second) = if placement_first {
        (placement, root)
    } else {
        (root, placement)
    };
    bytes.extend(first);
    bytes.extend_from_slice(&48_u32.to_le_bytes());
    bytes.extend(second);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(inline_cross_document(contained, PLACEMENT, ROLE));
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.push(0);
    bytes.push(0);
    for value in transform.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    bytes.extend(inline_local(8506, PLACEMENT));
    bytes.extend(inline_cross_document(contained, PLACEMENT, ROLE));
    bytes.extend(inline_cross_document(152, ROOT_COMPONENT, NESTED_ROLE));
    bytes.extend(local_reference(3));
    bytes
}

fn nested_retained(entity_id: u64, contained: u64, world: [[f64; 4]; 4]) -> XrefContainedPose {
    let magic: Option<u32> = None;
    let bytes = nested_carrier(entity_id, false, contained, world);
    let pose = crate::xref::contained_component_path_pose(&bytes, magic).unwrap();
    XrefContainedPose {
        carrier_record_index: pose.carrier_record_index,
        frame_bytes: bytes.into(),
        contained_record_index: pose.contained_record_index,
        owner_record_indices: pose.owner_record_indices,
        transform: pose.transform.map(|rows| rows.try_into().unwrap()),
    }
}

#[test]
fn a_nested_design_carrier_names_its_placement_and_the_nested_occurrence() {
    let magic: Option<u32> = None;
    let world = translation(20.0, 0.3, 5.0);
    for placement_first in [false, true] {
        let bytes = nested_carrier(11, placement_first, 166, world);
        let pose = crate::xref::contained_component_path_pose(&bytes, magic).unwrap();
        assert_eq!(
            (
                pose.carrier_record_index,
                pose.component_record_index,
                pose.contained_record_index,
                pose.placement_record_index,
                pose.owner_record_indices.as_slice(),
                pose.role.as_str(),
            ),
            (11, 152, 166, 8506, &[][..], ROLE)
        );
        assert_eq!(pose.transform, Some(world));
        // The carrier is not the external occurrence's own pose.
        assert!(crate::xref::component_path_pose(&bytes, magic).is_none());
        assert!(crate::xref::owned_component_path_pose(&bytes, magic).is_none());

        // Every proper prefix and a trailing byte leave no carrier.
        for length in 0..bytes.len() {
            assert!(
                crate::xref::contained_component_path_pose(&bytes[..length], magic).is_none(),
                "prefix of {length} bytes"
            );
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(crate::xref::contained_component_path_pose(&longer, magic).is_none());
    }
}

#[test]
fn a_nested_design_carrier_distinguishes_target_types_and_role_forms() {
    let magic: Option<u32> = None;
    let world = translation(20.0, 0.3, 5.0);
    let good = nested_carrier(11, false, 166, world);
    assert!(crate::xref::contained_component_path_pose(&good, magic).is_some());
    let replace = |from: &[u8], to: &[u8]| {
        let at = good
            .windows(from.len())
            .position(|window| window == from)
            .unwrap();
        let mut changed = good.clone();
        changed.splice(at..at + from.len(), to.iter().copied());
        changed
    };
    let same_role = replace(
        &NESTED_ROLE
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
        &ROLE
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    let ordered = crate::xref::contained_component_path_pose(&same_role, magic).unwrap();
    assert_eq!(ordered.contained_record_index, 166);
    assert_eq!(ordered.placement_record_index, 8506);
    // The component element is not a root component.
    let not_root = replace(ROOT_COMPONENT.as_bytes(), PLACEMENT.as_bytes());
    assert!(crate::xref::contained_component_path_pose(&not_root, magic).is_none());
    // The document element is neither an untyped nor a placement-typed reference.
    let mut other_local = nested_carrier(11, false, 166, world);
    let at = other_local
        .windows(PLACEMENT.len())
        .position(|window| window == PLACEMENT.as_bytes())
        .unwrap();
    other_local[at..at + ROOT_COMPONENT.len()].copy_from_slice(ROOT_COMPONENT.as_bytes());
    assert!(crate::xref::contained_component_path_pose(&other_local, magic).is_none());
    // A zero placement target names no record.
    let zero_target = nested_carrier(11, false, 0, world);
    assert!(crate::xref::contained_component_path_pose(&zero_target, magic).is_none());
    // A matrix that is not a rigid motion is no pose.
    let mut scaled = world;
    scaled[0][0] = 2.0;
    assert!(crate::xref::contained_component_path_pose(
        &nested_carrier(11, false, 166, scaled),
        magic
    )
    .is_none());
}

#[test]
fn nested_design_carriers_place_an_external_occurrence_off_the_identity() {
    let poses = || {
        vec![
            nested_retained(11, 496, translation(23.0, 4.0, 0.0)),
            nested_retained(12, 497, translation(17.0, 4.0, 0.0)),
        ]
    };
    assert!(crate::xref::contained_poses_match(&reference(poses())));
    // The parts' world poses put the instance at 200 mm, 40 mm: 195 mm below the owner at 5 mm.
    let (transform, losses, left_identity) = place_reference_stored_at(reference(poses()), 0.0);
    let rows = transform.rows();
    assert!(
        (rows[0][3] - 195.0).abs() < 1e-9 && (rows[1][3] - 40.0).abs() < 1e-9,
        "{rows:?}"
    );
    assert_eq!(losses, [F3dLossCode::XrefContainedPose.kind()]);
    assert_eq!(left_identity, 1);

    // An instance stored away from the identity is not counted as having left it.
    let (_, _, left_identity) = place_reference_stored_at(reference(poses()), 10.0);
    assert_eq!(left_identity, 0);

    // Carriers that give two poses leave the stored placement.
    let mut disagreeing = poses();
    disagreeing[1] = nested_retained(12, 497, translation(18.0, 4.0, 0.0));
    let (transform, losses, left_identity) = place_reference_stored_at(reference(disagreeing), 0.0);
    assert!(transform.rows()[0][3].abs() < 1e-9);
    assert_eq!(losses, [F3dLossCode::XrefContainedPoseDisagrees.kind()]);
    assert_eq!(left_identity, 0);

    // A moved matrix no longer matches its retained carrier bytes.
    let mut moved = reference(poses());
    moved.contained_poses[0].transform = Some(translation(24.0, 4.0, 0.0).try_into().unwrap());
    assert!(!crate::xref::contained_poses_match(&moved));
}

#[test]
fn typed_component_paths_require_the_element_role_and_complete_tail() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    for typed in [false, true] {
        for placement_first in [false, true] {
            let mut bytes = carrier(
                11,
                ROOT_COMPONENT,
                ROLE,
                496,
                &[9222, 9300],
                translation(23.0, 4.0, 0.0),
            );
            let mut members = Vec::new();
            let mut end = 24;
            for _ in 0..5 {
                let reference = crate::bytes::take_owned_reference(&bytes, &mut end).unwrap();
                let discriminator = cadmpeg_core::decode::View::u32_le_at(&bytes, end).unwrap();
                end += 4;
                members.push((reference, discriminator));
            }
            if placement_first {
                members.swap(0, 2);
                members.swap(1, 2);
            }
            let mut elements = Vec::new();
            for (reference, discriminator) in members {
                if let Some((target, _)) = reference.local() {
                    elements.extend(if typed {
                        inline_local(target, PLACEMENT)
                    } else {
                        local_reference(target)
                    });
                } else {
                    let crate::bytes::Reference::CrossDocument {
                        target,
                        target_type_guid,
                        link_name,
                        ..
                    } = reference
                    else {
                        unreachable!()
                    };
                    elements.extend(crate::test_support::typed_cross_document_reference(
                        target,
                        &target_type_guid,
                        &link_name,
                    ));
                }
                elements.extend_from_slice(&discriminator.to_le_bytes());
            }
            bytes.splice(24..end, elements);
            let pose = crate::xref::contained_component_path_pose(&bytes, magic).unwrap();
            assert_eq!(pose.contained_record_index, 496);
            assert_eq!(pose.placement_record_index, 8506);
            assert_eq!(pose.owner_record_indices, [9222, 9300]);
            if typed {
                for target in [8506, 9222, 9300] {
                    let expected = inline_local(target, PLACEMENT);
                    let at = bytes
                        .windows(expected.len())
                        .position(|window| window == expected)
                        .unwrap();
                    let mut bad = bytes.clone();
                    bad.splice(
                        at..at + expected.len(),
                        inline_local(target, ROOT_COMPONENT),
                    );
                    assert!(crate::xref::contained_component_path_pose(&bad, magic).is_none());
                }
            }
            bytes.push(0);
            assert!(crate::xref::contained_component_path_pose(&bytes, magic).is_none());
        }
    }
}

#[test]
fn independent_contained_world_poses_use_the_corrected_insertion_frame() {
    let (ir, losses, _) = placed_reference(
        with_owned(
            vec![
                retained(12, 496, &[9222], translation(33.0, 4.0, 0.0)),
                retained(13, 497, &[9222], translation(12.0, 7.0, 0.0)),
            ],
            vec![typed_owned(11, &[9222], translation(20.0, 4.0, 0.0))],
        ),
        10.0,
    );
    let insertion = ir.model.occurrences[3].transform;
    let first = ir.model.occurrences[5].transform;
    let second = ir.model.occurrences[6].transform;
    assert!((insertion.rows()[0][3] - 195.0).abs() < 1e-9);
    assert!((first.rows()[0][3] - 130.0).abs() < 1e-9);
    assert!((first.rows()[1][3]).abs() < 1e-9);
    assert!((second.rows()[0][3] + 80.0).abs() < 1e-9);
    assert!((second.rows()[1][3] - 30.0).abs() < 1e-9);
    assert_eq!(
        losses,
        [
            F3dLossCode::XrefContainedPose.kind(),
            F3dLossCode::XrefOwnedPose.kind()
        ]
    );
}

#[test]
fn nested_component_paths_keep_typed_owners_and_refuse_owner_type_mismatch() {
    for placement_first in [false, true] {
        let mut bytes = nested_carrier(11, placement_first, 496, translation(24.0, 4.0, 0.0));
        let mut end = 24;
        for _ in 0..3 {
            crate::bytes::take_reference(&bytes, &mut end).unwrap();
            end += 4;
        }
        bytes[20..24].copy_from_slice(&4_u32.to_le_bytes());
        let mut owner = inline_local(9222, PLACEMENT);
        owner.extend_from_slice(&0_u32.to_le_bytes());
        bytes.splice(end..end, owner);
        let pose = crate::xref::contained_component_path_pose(&bytes, None).unwrap();
        assert_eq!(pose.owner_record_indices, [9222]);
        let expected = inline_local(9222, PLACEMENT);
        bytes.splice(
            end..end + expected.len(),
            inline_local(9222, ROOT_COMPONENT),
        );
        assert!(crate::xref::contained_component_path_pose(&bytes, None).is_none());
    }
}

#[test]
fn conflicting_contained_carriers_keep_the_child_and_place_the_insertion() {
    let (ir, losses, _) = placed_reference(
        with_owned(
            vec![
                retained(12, 496, &[9222], translation(33.0, 4.0, 0.0)),
                retained(13, 496, &[9222], translation(34.0, 4.0, 0.0)),
            ],
            vec![typed_owned(11, &[9222], translation(20.0, 4.0, 0.0))],
        ),
        10.0,
    );
    assert!((ir.model.occurrences[3].transform.rows()[0][3] - 195.0).abs() < 1e-9);
    assert!((ir.model.occurrences[5].transform.rows()[0][3] - 30.0).abs() < 1e-9);
    assert_eq!(
        losses,
        [
            F3dLossCode::XrefContainedPoseDisagrees.kind(),
            F3dLossCode::XrefOwnedPose.kind()
        ]
    );
}

#[test]
fn repeated_contained_design_records_keep_an_ambiguous_target_native() {
    let (ir, losses, _) = placed_reference_edit(
        with_owned(
            vec![retained(12, 496, &[9222], translation(33.0, 4.0, 0.0))],
            vec![typed_owned(11, &[9222], translation(20.0, 4.0, 0.0))],
        ),
        10.0,
        |ir| {
            let mut repeated = ir.model.occurrences[5].clone();
            repeated.id =
                OccurrenceId::mint("f3d:xref/role/occurrence-0/model:occurrence#repeated-child")
                    .unwrap();
            ir.model.occurrences.push(repeated);
        },
    );
    assert!((ir.model.occurrences[5].transform.rows()[0][3] - 30.0).abs() < 1e-9);
    assert!((ir.model.occurrences[7].transform.rows()[0][3] - 30.0).abs() < 1e-9);
    assert_eq!(
        losses,
        [
            F3dLossCode::XrefOwnedPose.kind(),
            F3dLossCode::XrefContainedPoseNative.kind()
        ]
    );
}

#[test]
fn repeated_insertions_require_a_unique_owner_qualified_component_path() {
    let (ir, losses, _) = placed_reference_edit(
        with_owned(
            vec![retained(12, 496, &[9222], translation(33.0, 4.0, 0.0))],
            vec![typed_owned(11, &[9222], translation(20.0, 4.0, 0.0))],
        ),
        10.0,
        |ir| {
            let mut repeated = ir.model.occurrences[3].clone();
            repeated.id = OccurrenceId::mint("f3d:model:occurrence#repeated-insertion").unwrap();
            ir.model.occurrences.push(repeated);
        },
    );
    assert!((ir.model.occurrences[3].transform.rows()[0][3] - 10.0).abs() < 1e-9);
    assert!((ir.model.occurrences[7].transform.rows()[0][3] - 10.0).abs() < 1e-9);
    assert_eq!(
        losses,
        [
            F3dLossCode::XrefOwnedPoseNative.kind(),
            F3dLossCode::XrefContainedPoseNative.kind()
        ]
    );
}

fn locals_first(bytes: &[u8]) -> Vec<u8> {
    let count = cadmpeg_core::decode::View::u32_le_at(bytes, 20).unwrap();
    let mut at = 24;
    let mut local = Vec::new();
    let mut external = Vec::new();
    for _ in 0..count {
        let start = at;
        let reference = crate::bytes::take_reference(bytes, &mut at).unwrap();
        at += 4;
        if reference.local().is_some() {
            local.extend_from_slice(&bytes[start..at]);
        } else {
            external.extend_from_slice(&bytes[start..at]);
        }
    }
    let mut result = bytes[..24].to_vec();
    result.extend(local);
    result.extend(external);
    result.extend_from_slice(&bytes[at..]);
    result
}

#[test]
fn component_paths_admit_local_owners_before_the_cross_document_elements() {
    let magic = Some(crate::metastream::MODERN_SERIALIZER_MAGIC);
    let own = typed_owned(11, &[9222, 9300], translation(20.0, 4.0, 0.0));
    let own_bytes = locals_first(&own.frame_bytes);
    let parsed = crate::xref::owned_component_path_pose(&own_bytes, magic).unwrap();
    assert_eq!(parsed.placement_record_index, 8506);
    assert_eq!(parsed.owner_record_indices, [9222, 9300]);
    let bytes = locals_first(&carrier(
        12,
        ROOT_COMPONENT,
        ROLE,
        496,
        &[9222, 9300],
        translation(33.0, 4.0, 0.0),
    ));
    let parsed = crate::xref::contained_component_path_pose(&bytes, magic).unwrap();
    assert_eq!(parsed.contained_record_index, 496);
    assert_eq!(parsed.owner_record_indices, [9222, 9300]);
}

#[test]
fn contained_component_paths_follow_the_expanded_scope_of_a_shared_design() {
    let (ir, losses, _) = placed_reference_edit(
        with_owned(
            vec![retained(12, 496, &[9222], translation(33.0, 4.0, 0.0))],
            vec![typed_owned(11, &[9222], translation(20.0, 4.0, 0.0))],
        ),
        10.0,
        |ir| {
            ir.model.occurrences[5].native_ref = Some(
                "f3d:xref/first/xref/shared/Design:design-component-occurrence#496".to_owned(),
            );
        },
    );
    assert!((ir.model.occurrences[5].transform.rows()[0][3] - 130.0).abs() < 1e-9);
    assert_eq!(
        losses,
        [
            F3dLossCode::XrefContainedPose.kind(),
            F3dLossCode::XrefOwnedPose.kind()
        ]
    );
}

#[test]
fn contained_world_poses_use_the_inverse_of_the_rotated_insertion_frame() {
    let mut pose = translation(20.0, 4.0, 0.0);
    pose[0][0] = 0.0;
    pose[0][1] = -1.0;
    pose[1][0] = 1.0;
    pose[1][1] = 0.0;
    let (ir, _, _) = placed_reference(
        with_owned(
            vec![retained(12, 496, &[9222], translation(33.0, 4.0, 0.0))],
            vec![typed_owned(11, &[9222], pose)],
        ),
        10.0,
    );
    let local = ir.model.occurrences[5].transform.rows();
    assert!(local[0][3].abs() < 1e-9);
    assert!((local[1][3] + 130.0).abs() < 1e-9);
}

#[test]
fn contained_component_paths_refuse_two_member_scopes_below_one_insertion() {
    let (ir, losses, _) = placed_reference_edit(
        with_owned(
            vec![retained(12, 496, &[9222], translation(33.0, 4.0, 0.0))],
            vec![typed_owned(11, &[9222], translation(20.0, 4.0, 0.0))],
        ),
        10.0,
        |ir| {
            ir.model.occurrences.push(occurrence(
                "f3d:xref/other/model:occurrence#root",
                Some("f3d:model:occurrence#owner:xref-0-0"),
                None,
                0.0,
            ));
        },
    );
    assert!((ir.model.occurrences[5].transform.rows()[0][3] - 30.0).abs() < 1e-9);
    assert_eq!(
        losses,
        [
            F3dLossCode::XrefOwnedPose.kind(),
            F3dLossCode::XrefContainedPoseNative.kind()
        ]
    );
}

#[test]
fn owned_nested_component_paths_keep_the_saved_state_native() {
    let mut bytes = nested_carrier(13, true, 497, translation(12.0, 7.0, 0.0));
    let mut at = 24;
    for _ in 0..3 {
        crate::bytes::take_reference(&bytes, &mut at).unwrap();
        at += 4;
    }
    bytes[20..24].copy_from_slice(&4_u32.to_le_bytes());
    let mut owner = inline_local(9222, PLACEMENT);
    owner.extend_from_slice(&0_u32.to_le_bytes());
    bytes.splice(at..at, owner);
    let pose = crate::xref::contained_component_path_pose(&bytes, None).unwrap();
    let nested = XrefContainedPose {
        carrier_record_index: pose.carrier_record_index,
        frame_bytes: bytes.into(),
        contained_record_index: pose.contained_record_index,
        owner_record_indices: pose.owner_record_indices,
        transform: pose.transform.map(|rows| rows.try_into().unwrap()),
    };
    let (ir, losses, _) = placed_reference(
        with_owned(
            vec![
                retained(12, 496, &[9222], translation(33.0, 4.0, 0.0)),
                nested,
            ],
            vec![typed_owned(11, &[9222], translation(20.0, 4.0, 0.0))],
        ),
        10.0,
    );
    assert!((ir.model.occurrences[5].transform.rows()[0][3] - 130.0).abs() < 1e-9);
    assert!((ir.model.occurrences[6].transform.rows()[0][3] + 30.0).abs() < 1e-9);
    assert!((ir.model.occurrences[6].transform.rows()[1][3]).abs() < 1e-9);
    assert_eq!(
        losses,
        [
            F3dLossCode::XrefContainedPose.kind(),
            F3dLossCode::XrefOwnedPose.kind(),
            F3dLossCode::XrefContainedPoseNative.kind()
        ]
    );
}

#[test]
fn typed_contained_pose_without_an_own_carrier_composes_in_each_member_instance() {
    const REFERENCE: &str = "f3d:xref/source/occurrence-0/xref:reference#0-occurrence-0";
    const PART: &str = "f3d:xref/source/occurrence-0/xref/part/Design/BulkStream.dat:design-component-occurrence#496";
    let mut pose = retained(12, 496, &[], translation(3.0, 4.0, 5.0));
    let mut bytes = pose.frame_bytes.to_vec();
    let mut at = 24;
    for _ in 0..2 {
        crate::bytes::take_reference(&bytes, &mut at).unwrap();
        at += 4;
    }
    bytes.splice(at..at + 11, inline_local(8506, PLACEMENT));
    pose.frame_bytes = bytes.into();
    let mut reference = reference(vec![pose]);
    reference.id = REFERENCE.to_owned();
    assert!(crate::xref::contained_poses_match(&reference));
    let table = crate::xref::XrefTable {
        references: vec![reference.clone()],
        ..Default::default()
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    for instance in 0..2 {
        let boundary = format!("f3d:model:occurrence#source-{instance}");
        let insertion = format!("f3d:xref/source-{instance}/model:occurrence#insertion");
        let root = format!("f3d:xref/source-{instance}/xref/part/model:occurrence#root");
        let leaf = format!("f3d:xref/source-{instance}/xref/part/model:occurrence#leaf");
        ir.model.occurrences.extend([
            occurrence(&boundary, None, None, f64::from(instance) * 100.0),
            occurrence(&insertion, Some(&boundary), Some(REFERENCE), 0.0),
            occurrence(&root, Some(&insertion), None, 0.0),
            occurrence(&leaf, Some(&root), Some(PART), 0.0),
        ]);
    }
    ir.model.occurrences[4].transform = Transform::affine([
        [0.0, -1.0, 0.0, 100.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    crate::test_support::native_test::update_f3d_native(&mut ir, |native| {
        native.design_component_occurrences = vec![local_carrier(PART, 496)];
        native.xref_references = vec![reference];
    });
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut protected = std::collections::BTreeSet::new();
    let mut losses = Vec::new();
    for index in [0, 4] {
        let boundary = ir.model.occurrences[index].id.clone();
        super::super::place_contained_poses_with_protected(
            &ctx,
            &mut ir,
            &table,
            &mut losses,
            &mut protected,
            super::super::ContainedPass::Ordered,
            Some(&boundary),
        )
        .unwrap();
    }
    for index in [0, 4] {
        assert_eq!(
            ir.model.occurrences[index + 1].transform,
            Transform::identity()
        );
        let local = ir.model.occurrences[index + 3].transform.rows();
        for (row, expected) in local.iter().zip([30.0, 40.0, 50.0]) {
            assert!((row[3] - expected).abs() < 1e-9);
        }
        let world = ir.model.occurrences[index]
            .transform
            .compose(ir.model.occurrences[index + 3].transform)
            .unwrap();
        let expected = if index == 0 {
            [30.0, 40.0, 50.0]
        } else {
            [60.0, 30.0, 50.0]
        };
        for (row, expected) in world.rows().iter().zip(expected) {
            assert!((row[3] - expected).abs() < 1e-9);
        }
    }
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .all(|loss| loss.code == F3dLossCode::XrefContainedPose.kind()));
    let absent = OccurrenceId::mint("f3d:model:occurrence#absent-source").unwrap();
    assert!(super::super::place_contained_poses_with_protected(
        &ctx,
        &mut ir,
        &table,
        &mut losses,
        &mut protected,
        super::super::ContainedPass::Ordered,
        Some(&absent),
    )
    .is_err());
}

fn close_with_target(mut pose: XrefContainedPose, target: u64) -> XrefContainedPose {
    let mut bytes = pose.frame_bytes.to_vec();
    let count = cadmpeg_core::decode::View::u32_le_at(&bytes, 20).unwrap();
    let mut at = 24;
    for _ in 0..count {
        crate::bytes::take_reference(&bytes, &mut at).unwrap();
        at += 4;
    }
    assert_eq!(&bytes[at..at + 2], &[0, 0]);
    at += 2 + std::mem::size_of::<[[f64; 4]; 4]>();
    let start = at;
    let count = cadmpeg_core::decode::View::u32_le_at(&bytes, at).unwrap();
    at += 4;
    for _ in 0..count {
        crate::bytes::take_reference(&bytes, &mut at).unwrap();
    }
    let mut closing = 2_u32.to_le_bytes().to_vec();
    closing.extend(local_reference(8506));
    closing.extend(typed_cross_document_reference(target, PLACEMENT, ROLE));
    bytes.splice(start..at, closing);
    pose.frame_bytes = bytes.into();
    pose
}

#[test]
fn component_path_chain_keeps_a_child_local_pose_when_its_source_frame_agrees() {
    let mut source = translation(5.0, 0.0, 0.0);
    source[0][0] = 0.0;
    source[0][1] = -1.0;
    source[1][0] = 1.0;
    source[1][1] = 0.0;
    let mut world = source;
    world[1][3] = 3.0;
    let mut pose = close_with_target(retained(12, 496, &[], world), 496);
    let mut bytes = pose.frame_bytes.to_vec();
    let mut at = 24;
    for _ in 0..2 {
        crate::bytes::take_reference(&bytes, &mut at).unwrap();
        at += 4;
    }
    bytes.splice(at..at + 11, inline_local(8506, PLACEMENT));
    pose.frame_bytes = bytes.into();
    let mut reference = reference(vec![pose]);
    reference.transform = Some(source.try_into().unwrap());
    let mut current_world = source;
    current_world[0][3] = 10.0;
    current_world[1][3] = 20.0;
    let current_bytes = owned_carrier(13, &[], current_world);
    let current = crate::xref::component_path_pose(
        &current_bytes,
        Some(crate::metastream::MODERN_SERIALIZER_MAGIC),
    )
    .unwrap();
    reference.current_pose = Some(crate::records::xref::XrefCurrentPose {
        carrier_record_index: current.carrier_record_index,
        frame_bytes: current_bytes.into(),
        transform: current.transform.map(|rows| rows.try_into().unwrap()),
    });
    let (ir, losses, _) = placed_reference_edit(reference, 100.0, |ir| {
        ir.model.occurrences[3].parent = ir.model.occurrences[0].parent.clone();
        ir.model.occurrences[3].transform = Transform::affine([
            [0.0, -1.0, 0.0, 100.0],
            [1.0, 0.0, 0.0, 200.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .unwrap();
    });
    let world = ir.model.occurrences[3]
        .transform
        .compose(ir.model.occurrences[5].transform)
        .unwrap();
    for (row, expected) in world.rows().iter().zip([100.0, 230.0, 0.0]) {
        assert!((row[3] - expected).abs() < 1e-9);
    }
    assert!((ir.model.occurrences[5].transform.rows()[0][3] - 30.0).abs() < 1e-9);
    assert_eq!(losses, [F3dLossCode::XrefContainedPose.kind()]);
}

#[test]
fn component_path_chain_places_a_nested_target_in_a_mixed_reference() {
    let (ir, losses, _) = placed_reference(
        reference(vec![
            retained(11, 496, &[], translation(5.0, 0.0, 0.0)),
            nested_retained(12, 497, translation(9.0, 0.0, 0.0)),
        ]),
        10.0,
    );
    let world = ir.model.occurrences[2]
        .transform
        .compose(ir.model.occurrences[3].transform)
        .and_then(|world| world.compose(ir.model.occurrences[6].transform))
        .unwrap();
    assert!((world.rows()[0][3] - 90.0).abs() < 1e-9);
    assert_eq!(
        losses,
        [
            F3dLossCode::XrefContainedPose.kind(),
            F3dLossCode::XrefContainedPose.kind()
        ]
    );
}

#[test]
fn component_path_chain_requires_the_nested_target_in_a_mixed_reference() {
    let pose = close_with_target(nested_retained(12, 497, translation(9.0, 0.0, 0.0)), 498);
    let (ir, losses, _) = placed_reference(
        reference(vec![
            retained(11, 496, &[], translation(5.0, 0.0, 0.0)),
            pose,
        ]),
        10.0,
    );
    assert!((ir.model.occurrences[6].transform.rows()[0][3] + 30.0).abs() < 1e-9);
    assert!(losses.contains(&F3dLossCode::XrefContainedPoseNative.kind()));
}
