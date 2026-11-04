// SPDX-License-Identifier: Apache-2.0
use super::surface_patch_normal;
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};
use crate::records::feature::surface_ops::{
    DesignPatchContinuity, DesignSurfacePatchBoundary, DesignSurfacePatchNormalState,
};
use crate::records::identity::Located;

const PATCH_GUID: &str = "BF77AE1A-E8F6-43AB-A325-A2C984AF1C6B";

#[test]
fn source_normal_prefixes_require_the_semantic_version_and_bounded_boolean() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#353",
        DesignFeatureKind::SurfacePatch,
        353,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 17;
            draft.reference_count_offset = 26;
            draft.paired_byte_offset = 145;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    for (hex, flag) in [
        (
            "030000003333336101000000000000000000000001000000000000000000000004000000",
            false,
        ),
        (
            "030000003332376101000000000000000000000001000000000000000000000104000000",
            true,
        ),
    ] {
        let raw: cadmpeg_ir::native::bytes::NativeBytes<Vec<u8>> =
            serde_json::from_value(serde_json::json!(hex)).unwrap();
        assert_eq!(raw.len(), 36);
        let mut bytes = vec![0; 145];
        bytes[17..53].copy_from_slice(&raw);
        let normal = surface_patch_normal(&bytes, &scope, Some((PATCH_GUID, 7)));
        assert_eq!(
            normal,
            Some(DesignSurfacePatchNormalState::Explicit {
                reversed: Located {
                    value: flag,
                    offset: 48
                }
            })
        );
        for version in [1, 5, 6] {
            assert_eq!(
                surface_patch_normal(&bytes, &scope, Some((PATCH_GUID, version))),
                None
            );
        }
        assert_eq!(
            surface_patch_normal(
                &bytes,
                &scope,
                Some(("00000000-0000-0000-0000-000000000000", 7))
            ),
            Some(DesignSurfacePatchNormalState::Unsupported)
        );
        assert_eq!(
            surface_patch_normal(&bytes, &scope, None),
            Some(DesignSurfacePatchNormalState::Unsupported)
        );
        for version in [0, 2, 3, 4, 8] {
            assert_eq!(
                surface_patch_normal(&bytes, &scope, Some((PATCH_GUID, version))),
                Some(DesignSurfacePatchNormalState::Unsupported)
            );
        }
        for (at, value) in [(48, 2), (49, 3)] {
            let mut invalid = bytes.clone();
            invalid[at] = value;
            assert_eq!(
                surface_patch_normal(&invalid, &scope, Some((PATCH_GUID, 7))),
                Some(DesignSurfacePatchNormalState::Unsupported)
            );
        }
        assert_eq!(
            surface_patch_normal(&bytes[..52], &scope, Some((PATCH_GUID, 7))),
            Some(DesignSurfacePatchNormalState::Unsupported)
        );
        let other = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#353",
            DesignFeatureKind::Sketch,
            353,
        );
        assert_eq!(
            surface_patch_normal(&bytes, &other, Some((PATCH_GUID, 7))),
            None
        );
    }
}

#[test]
fn native_normal_state_composes_with_boundaries_and_validates_its_owner_and_offset() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#353",
        DesignFeatureKind::SurfacePatch,
        353,
    );
    if let DesignScopePayloadMut::SurfacePatch(patch) = scope.payload_mut() {
        patch.normal = Some(DesignSurfacePatchNormalState::Explicit {
            reversed: Located {
                value: true,
                offset: 31,
            },
        });
        patch.boundaries.push(DesignSurfacePatchBoundary {
            scope_reference_ordinal: 0,
            record_index: 360,
            is_seed_selection: false,
            continuity: DesignPatchContinuity::Tangent,
            flip: 2,
            scale: crate::test_support::real(-1.0),
            model_reference: 353,
        });
    }
    let wire = serde_json::to_value(&scope).unwrap();
    assert_eq!(
        serde_json::from_value::<DesignParameterScope>(wire.clone()).unwrap(),
        scope
    );
    for field in ["kind", "surface_patch_normal"] {
        let mut invalid = wire.clone();
        invalid[field] = if field == "kind" {
            serde_json::json!("Sketch")
        } else {
            serde_json::Value::Null
        };
        assert!(serde_json::from_value::<DesignParameterScope>(invalid).is_err());
    }
    let mut invalid = wire;
    invalid["surface_patch_normal"]["reversed"]["offset"] = serde_json::json!(32);
    assert!(serde_json::from_value::<DesignParameterScope>(invalid).is_err());
}

#[test]
fn typed_local_boundary_reference_preserves_the_group_and_rejects_nonlocal_or_truncated_forms() {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"999", 34397);
    bytes.resize(38, 0);
    bytes[26..30].copy_from_slice(&2_u32.to_le_bytes());
    bytes[30..38].copy_from_slice(&(-1.0_f64).to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&34390_u64.to_le_bytes());
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(b"2CA5A1CD-C99B-4C9B-91AF-57989148E841");
    bytes.extend_from_slice(&[0, 0]);
    let boundary = super::exact_surface_patch_boundary(&bytes, 0).unwrap();
    assert_eq!(boundary.model_reference, 34390);
    assert_eq!(boundary.continuity, DesignPatchContinuity::Connected);
    assert_eq!(boundary.flip, 2);
    assert_eq!(boundary.scale.get(), -1.0);
    assert!(super::exact_surface_patch_boundary(&bytes[..88], 0).is_none());
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&bytes);
    assert!(super::surface_patch_boundaries(&bytes, &records, &[34397]).is_empty());
    let mut unpaired = bytes[..48].to_vec();
    crate::test_support::indexed_header(&mut unpaired, *b"998", 800);
    unpaired.resize(128, 0);
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&unpaired);
    assert!(super::surface_patch_boundaries(&unpaired, &records, &[34397]).is_empty());
    let mut truncated = bytes[..88].to_vec();
    crate::test_support::indexed_header(&mut truncated, *b"997", 34397);
    let records = crate::design::decode::sketch::IndexedRecordOffsets::build(&truncated);
    assert!(super::surface_patch_boundaries(&truncated, &records, &[34397]).is_empty());
    bytes[88] = 1;
    bytes.extend_from_slice(&7_u32.to_le_bytes());
    assert!(super::exact_surface_patch_boundary(&bytes, 0).is_none());
}
