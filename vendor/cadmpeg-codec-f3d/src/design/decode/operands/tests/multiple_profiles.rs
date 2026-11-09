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

#[test]
fn compact_profile_flags_are_retained_and_report_the_assumption() {
    for (bytes, flags) in [
        ([1, 0], [true, false]),
        ([0, 1], [false, true]),
        ([1, 1], [true, true]),
    ] {
        for tail_length in [89, 93] {
            let (archive, mut scope, headers, entities) =
                crate::test_support::multiple_profiles::fixture_with_flags(
                    1,
                    Some((bytes, tail_length)),
                );
            crate::test_support::zip_test::with_scan(&archive, |scan| {
                let losses = bind_sketch_profiles(
                    scan,
                    std::slice::from_mut(&mut scope),
                    &headers,
                    &entities,
                )
                .unwrap();
                assert_eq!(losses.len(), 1);
                assert_eq!(
                    losses[0].code.local_code(),
                    "feature.sketch-profile-flags-assumed"
                );
                assert_eq!(
                    losses[0].message,
                    format!(
                        "Sketch-profile operand record 100 has compact tail flags [{}, {}]; the flags are assumed not to change the profile interpretation.",
                        bytes[0], bytes[1]
                    )
                );
                let profile = scope.extrude_profile().unwrap();
                assert_eq!(profile.entity_id.as_str(), "0_172");
                assert_eq!(profile.compact_tail_flags, Some(flags));
                let native = serde_json::to_value(profile).unwrap();
                assert_eq!(native["compact_tail_flags"], serde_json::json!(flags));
                let decoded: crate::records::topology::sketch_profile::DesignSketchProfileOperand =
                    serde_json::from_value(native).unwrap();
                assert_eq!(decoded.compact_tail_flags, Some(flags));
            });
        }
    }
}

#[test]
fn compact_profile_flags_require_binary_values() {
    for tail_length in [89, 93] {
        for flags in [[2, 0], [0, 2]] {
            let (archive, mut scope, headers, entities) =
                crate::test_support::multiple_profiles::fixture_with_flags(
                    1,
                    Some((flags, tail_length)),
                );
            crate::test_support::zip_test::with_scan(&archive, |scan| {
                let losses = bind_sketch_profiles(
                    scan,
                    std::slice::from_mut(&mut scope),
                    &headers,
                    &entities,
                )
                .unwrap();
                assert!(losses.is_empty());
                assert!(scope.extrude_profile().is_none());
            });
        }
    }
}

#[test]
fn compact_profile_zero_flags_bind_without_an_assumption_loss() {
    for tail_length in [89, 93] {
        let (archive, mut scope, headers, entities) =
            crate::test_support::multiple_profiles::fixture_with_flags(
                1,
                Some(([0, 0], tail_length)),
            );
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let losses =
                bind_sketch_profiles(scan, std::slice::from_mut(&mut scope), &headers, &entities)
                    .unwrap();
            assert!(losses.is_empty());
            let profile = scope.extrude_profile().unwrap();
            assert_eq!(profile.entity_id.as_str(), "0_172");
            assert_eq!(profile.compact_tail_flags, Some([false, false]));
        });
    }
}
