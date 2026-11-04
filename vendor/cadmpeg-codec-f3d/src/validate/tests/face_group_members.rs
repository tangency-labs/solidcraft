// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeSet, HashSet};

const STREAM: &str = "f3d:Design/BulkStream.dat";

fn unresolved_count(
    face_operands: &HashSet<(&str, u32, u32)>,
    identity_members: &HashSet<(&str, u32, u32)>,
) -> usize {
    let members = BTreeSet::from([(STREAM, 30, 31)]);
    let mut findings = Vec::new();
    super::super::validate_face_group_member_resolution(
        &mut findings,
        members,
        face_operands,
        identity_members,
        &[],
        &[],
    );
    findings.len()
}

#[test]
fn termination_member_resolves_through_its_group_identity() {
    let empty = HashSet::new();
    assert_eq!(unresolved_count(&empty, &empty), 1);
    let identity = HashSet::from([(STREAM, 30, 31)]);
    assert_eq!(unresolved_count(&empty, &identity), 0);
    let face = HashSet::from([(STREAM, 30, 31)]);
    assert_eq!(unresolved_count(&face, &empty), 0);
}

#[test]
fn identity_of_another_scope_does_not_resolve_the_member() {
    let empty = HashSet::new();
    let other_scope = HashSet::from([(STREAM, 40, 31)]);
    assert_eq!(unresolved_count(&empty, &other_scope), 1);
}

#[test]
fn split_direct_face_admits_the_reference_zero_and_two_tool_faces() {
    use crate::records::recipes::ConstructionRecipeKind;
    assert!(super::super::split_direct_face_admitted(
        1,
        ConstructionRecipeKind::BoundedFace
    ));
    assert!(super::super::split_direct_face_admitted(
        0,
        ConstructionRecipeKind::Face
    ));
    assert!(!super::super::split_direct_face_admitted(
        0,
        ConstructionRecipeKind::BoundedFace
    ));
    assert!(super::super::split_direct_face_admitted(
        2,
        ConstructionRecipeKind::Face
    ));
    assert!(!super::super::split_direct_face_admitted(
        2,
        ConstructionRecipeKind::BoundedFace
    ));
    assert!(!super::super::split_direct_face_admitted(
        3,
        ConstructionRecipeKind::Face
    ));
}

#[test]
fn retained_sketch_profiles_resolve_members_in_their_own_scope() {
    let (archive, mut scope, headers, entities) =
        crate::test_support::multiple_profiles::fixture(2);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let losses = crate::design::decode::operands::bind_sketch_profiles(
            scan,
            std::slice::from_mut(&mut scope),
            &headers,
            &entities,
        )
        .unwrap();
        assert_eq!(losses.len(), 1);
        let stream = crate::ids::native_stream(&scope.id).unwrap();
        for owner in [scope.record_index, scope.record_index + 1] {
            let mut findings = Vec::new();
            super::super::validate_face_group_member_resolution(
                &mut findings,
                BTreeSet::from([(stream, owner, 100), (stream, owner, 200)]),
                &HashSet::new(),
                &HashSet::new(),
                &[],
                std::slice::from_ref(&scope),
            );
            assert_eq!(
                findings.len(),
                if owner == scope.record_index { 0 } else { 2 }
            );
        }
    });
}
