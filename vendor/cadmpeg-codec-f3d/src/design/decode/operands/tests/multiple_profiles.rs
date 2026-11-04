// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::bind_sketch_profiles;
use crate::test_support::multiple_profiles::fixture;

#[test]
fn multiple_sketch_profiles_remain_native_with_one_loss() {
    let (archive, mut scope, headers, entities) = fixture(2);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let losses =
            bind_sketch_profiles(scan, std::slice::from_mut(&mut scope), &headers, &entities)
                .unwrap();
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.extrude-profile-group-native"
        );
        assert!(scope.extrude_profile().is_none());
        let profiles = scope.extrude_native_profiles().unwrap();
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.record_index)
                .collect::<Vec<_>>(),
            [100, 200]
        );
        assert_eq!(
            profiles
                .iter()
                .map(|profile| profile.entity_id.as_str())
                .collect::<Vec<_>>(),
            ["0_172", "0_173"]
        );
    });
}

#[test]
fn one_sketch_profile_binds_without_a_native_group_loss() {
    let (archive, mut scope, headers, entities) = fixture(1);
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        let losses =
            bind_sketch_profiles(scan, std::slice::from_mut(&mut scope), &headers, &entities)
                .unwrap();
        assert!(losses.is_empty());
        assert_eq!(scope.extrude_profile().unwrap().record_index, 100);
        assert!(scope.extrude_native_profiles().is_none());
    });
}
