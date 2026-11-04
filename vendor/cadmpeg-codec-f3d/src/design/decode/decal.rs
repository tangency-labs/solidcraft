// SPDX-License-Identifier: Apache-2.0
//! Parse exact raster and face bindings owned by Design `Decal` scopes.

use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::lp_utf16_bounded;
use crate::container::ContainerScan;
use crate::design::decode::image::embedded_image_asset;
use crate::design::decode::scopes::shared_frames::marked_reference;
use crate::design::decode::sketch::next_indexed_record_offset;

use crate::ids;
use crate::layout::design_decal_image_asset_record as decal_asset;
use crate::layout::design_decal_image_name_prefix as decal_name;
use crate::layout::design_decal_scope_prefix as decal_scope;
use crate::records::{
    decal::{DesignDecalAsset, DesignDecalImage},
    feature::scope::DesignParameterScope,
    topology::{
        body_recipe::DesignBodyRecipeOperand, construction::DesignConstructionOperandGroup,
    },
};
use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::assets::Asset;
use cadmpeg_ir::features::{
    DecalMapping, FaceSelection, Feature, FeatureDefinition, FeatureOperation,
};

const DECAL_TARGET_ROLE: crate::records::topology::extrude_selection::DesignOperandRole =
    crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A;

/// Decode every structurally complete Decal image record.
pub(crate) fn decode_decal_images(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
) -> Result<Vec<DesignDecalImage>, CodecError> {
    super::image::decode_scoped_images(
        scan,
        scopes,
        &crate::records::feature::scope::DesignFeatureKind::Decal,
        parse_decal_image,
        |image| image.id.as_str(),
    )
}

/// Project exact Decal image and face bindings into neutral features.
pub(crate) fn project_decal_images(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    images: &[DesignDecalImage],
    groups: &[DesignConstructionOperandGroup],
    operands: &[DesignBodyRecipeOperand],
    features: &mut [Feature],
) -> Result<Vec<Asset>, CodecError> {
    let mut assets = Vec::new();
    for image in images {
        if image.mapping_mode != crate::records::decal::DesignDecalMappingMode::FitToFaces {
            continue;
        }
        let native_stream = ids::native_stream(&image.id);
        let Some(scope) = scopes.iter().find(|scope| {
            scope.record_index == image.scope_record_index()
                && ids::native_stream(&scope.id) == native_stream
        }) else {
            continue;
        };
        let Some(group) = groups.iter().find(|group| {
            group.scope_record_index == scope.record_index
                && group.record_index == image.target_group_record_index
                && group.role() == DECAL_TARGET_ROLE
                && group.members().len() == 1
                && ids::native_stream(&group.id) == native_stream
        }) else {
            continue;
        };
        let Some(operand) = operands.iter().find(|operand| {
            operand.scope_record_index == scope.record_index
                && operand.owner.group() == Some((group.record_index, 0))
                && operand.record_index() == group.members()[0].value
                && ids::native_stream(&operand.id) == native_stream
        }) else {
            continue;
        };
        let mut faces: Vec<_> = operand
            .references()
            .iter()
            .flat_map(|reference| reference.candidate_faces.iter())
            .cloned()
            .collect();
        faces.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        faces.dedup();
        if faces.is_empty() {
            continue;
        }
        let Some(asset) = embedded_image_asset(ctx, scan, image.asset.name())? else {
            continue;
        };
        let feature_id = crate::ids::neutral_feature_id(scope);
        let Some(feature) = features.iter_mut().find(|feature| feature.id == feature_id) else {
            continue;
        };
        let asset_id = asset.id.clone();
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Decal {
                asset: asset_id,
                faces: FaceSelection::Resolved {
                    faces,
                    native: operand.id.clone(),
                },
                mapping: DecalMapping::FitToFaces,
                opacity: None,
            }));
        assets.push(asset);
    }
    assets.sort_by(|a, b| a.id.cmp(&b.id));
    assets.dedup_by(|a, b| a.id == b.id);
    Ok(assets)
}

fn parse_decal_image(
    bytes: &[u8],
    stream: &str,
    scope: &DesignParameterScope,
) -> Result<Option<DesignDecalImage>, CodecError> {
    let Ok(scope_at) = usize::try_from(scope.byte_offset()) else {
        return Ok(None);
    };
    parse_decal_image_frame(bytes, stream, scope.record_index, scope_at)
}

fn parse_decal_image_frame(
    bytes: &[u8],
    stream: &str,
    scope_record_index: u32,
    scope_at: usize,
) -> Result<Option<DesignDecalImage>, CodecError> {
    let parsed = (|| {
        if bytes
            .get(scope_at + decal_scope::ZERO_RUN_10..scope_at + decal_scope::ASSET_REFERENCE)?
            != [0; 10]
        {
            return None;
        }
        let asset_reference_at = scope_at + decal_scope::ASSET_REFERENCE;
        let asset_record_index = marked_reference(bytes, asset_reference_at)?;
        if bytes.get(
            scope_at + decal_scope::ASSET_REFERENCE_ZERO_RUN..scope_at + decal_scope::MAPPING_MODE,
        )? != [0; 6]
        {
            return None;
        }
        let mapping_mode_at = scope_at + decal_scope::MAPPING_MODE;
        let mapping_mode = *bytes.get(mapping_mode_at)?;
        let target_group_reference_at = scope_at + decal_scope::TARGET_GROUP_REFERENCE;
        let target_group_record_index = marked_reference(bytes, target_group_reference_at)?;
        if bytes
            .get(scope_at + decal_scope::TARGET_REFERENCE_ZERO_RUN..scope_at + decal_scope::LEN)?
            != [0; 6]
        {
            return None;
        }

        let mut position = 0;
        let mut asset_record = None;
        while let Some(asset_at) = next_indexed_record_offset(bytes, position) {
            position = asset_at.checked_add(1)?;
            if View::u32_le_at(bytes, asset_at + 7) != Some(asset_record_index) {
                continue;
            }
            let candidate = match parse_decal_asset_record(bytes, asset_at, asset_record_index) {
                Ok(Some(candidate)) => candidate,
                Ok(None) => continue,
                Err(error) => return Some(Err(error)),
            };
            if asset_record.replace(candidate).is_some() {
                return None;
            }
        }
        DesignDecalImage::new(
            ids::native_design_decal_image_id(stream, scope_at),
            crate::records::identity::Located {
                value: scope_record_index,
                offset: u64::try_from(scope_at).ok()?,
            },
            crate::records::decal::DesignDecalMappingMode::from_code(mapping_mode),
            target_group_record_index,
            asset_record?,
        )
        .ok()
        .map(Ok)
    })();
    parsed.transpose()
}

fn parse_decal_asset_record(
    bytes: &[u8],
    asset_at: usize,
    asset_record_index: u32,
) -> Result<Option<DesignDecalAsset>, CodecError> {
    let parsed = (|| {
        let (asset_class_tag, after_asset_tag) =
            lp_ascii_filtered_view(bytes, asset_at, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after_asset_tag)? != asset_record_index
            || bytes.get(
                asset_at + decal_asset::ZERO_RUN_8
                    ..asset_at + decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE,
            )? != [0; 8]
        {
            return None;
        }
        let asset_entity_reference_at = asset_at + decal_asset::DESIGN_ENTITY_SUFFIX_REFERENCE;
        let asset_entity_suffix = marked_reference(bytes, asset_entity_reference_at)?;
        if bytes.get(asset_at + decal_asset::ZERO_RUN_6..asset_at + decal_asset::LEN)? != [0; 6] {
            return None;
        }
        let name_at = next_indexed_record_offset(bytes, asset_at + decal_asset::ZERO_RUN_8)?;
        if name_at != asset_at + decal_asset::LEN {
            return None;
        }
        let (name_class_tag, after_name_tag) =
            lp_ascii_filtered_view(bytes, name_at, 0..=2000, u8::is_ascii_graphic)?;
        let name_record_index = View::u32_le_at(bytes, after_name_tag)?;
        if bytes.get(
            name_at + decal_name::ZERO_RUN_10..name_at + decal_name::ASSET_NAME_CODE_UNIT_COUNT,
        )? != [0; 10]
        {
            return None;
        }
        let (asset_name, after_asset_name) = lp_utf16_bounded(
            bytes,
            name_at + decal_name::ASSET_NAME_CODE_UNIT_COUNT,
            1..=1024,
        )?;
        let next_at = next_indexed_record_offset(bytes, name_at + decal_name::ZERO_RUN_10)?;
        if after_asset_name != next_at {
            return None;
        }
        let asset_class_tag = asset_class_tag.to_owned();
        let name_class_tag = name_class_tag.to_owned();

        DesignDecalAsset::new(
            [asset_class_tag, name_class_tag],
            [asset_record_index, name_record_index],
            u64::try_from(asset_at).ok()?,
            asset_entity_suffix,
            asset_name,
        )
        .ok()
        .map(Ok)
    })();
    parsed.transpose()
}

#[cfg(test)]
mod tests {
    use super::parse_decal_image_frame;
    use crate::records::decal::DesignDecalMappingMode;
    use crate::test_support::write_marked_reference;

    fn header(bytes: &mut [u8], at: usize, tag: [u8; 3], index: u32) {
        bytes[at..at + 4].copy_from_slice(&3u32.to_le_bytes());
        bytes[at + 4..at + 7].copy_from_slice(&tag);
        bytes[at + 7..at + 11].copy_from_slice(&index.to_le_bytes());
    }

    fn fixture() -> (Vec<u8>, usize) {
        let mut bytes = vec![0; 240];
        let asset_at = 0;
        let name_at = 30;
        let scope_at = 71;
        let end_at = 200;
        header(&mut bytes, asset_at, *b"258", 17);
        write_marked_reference(&mut bytes, asset_at + 19, 50);
        header(&mut bytes, name_at, *b"279", 18);
        let name = "mark.png".encode_utf16().collect::<Vec<_>>();
        bytes[name_at + 21..name_at + 25]
            .copy_from_slice(&u32::try_from(name.len()).unwrap().to_le_bytes());
        for (ordinal, unit) in name.into_iter().enumerate() {
            let at = name_at + 25 + ordinal * 2;
            bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        }
        header(&mut bytes, scope_at, *b"301", 23);
        write_marked_reference(&mut bytes, scope_at + 21, 17);
        bytes[scope_at + 32] = DesignDecalMappingMode::FitToFaces.code();
        write_marked_reference(&mut bytes, scope_at + 33, 24);
        header(&mut bytes, 150, *b"440", 17);
        header(&mut bytes, end_at, *b"302", 23);
        (bytes, scope_at)
    }

    #[test]
    fn decal_frame_decodes_image_mode_and_target() {
        let (bytes, scope_at) = fixture();
        let image = parse_decal_image_frame(&bytes, "Design/BulkStream.dat", 23, scope_at)
            .unwrap()
            .expect("complete synthetic Decal frame");
        assert_eq!(image.asset.record_index(), 17);
        assert_eq!(image.asset.entity_suffix(), 50);
        assert_eq!(image.asset.name(), "mark.png");
        assert_eq!(
            image.mapping_mode,
            crate::records::decal::DesignDecalMappingMode::FitToFaces
        );
        assert_eq!(image.target_group_record_index, 24);
        assert_eq!(
            crate::records::decal::DesignDecalAsset::primary_frame_length(),
            30
        );
        assert_eq!(image.asset.name_frame_length(), 41);
    }

    #[test]
    fn decal_frame_rejects_an_unframed_name() {
        let (mut bytes, scope_at) = fixture();
        bytes[scope_at..scope_at + 4].fill(0);
        assert!(
            parse_decal_image_frame(&bytes, "Design/BulkStream.dat", 23, scope_at)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn decal_projection_emits_one_asset() {
        use cadmpeg_ir::features::{
            Feature, FeatureDefinition, FeatureEvaluation, FeatureOperation, SketchFeatureBinding,
        };
        use std::collections::BTreeMap;
        use std::io::{Cursor, Write};
        use zip::CompressionMethod;

        const STREAM: &str = "Design/BulkStream.dat";
        const ENTRY: &str = "FusionAssetName[Active]/Design1/Images.BlobParts/mark.png";
        let (bytes, scope_at) = fixture();
        let image = parse_decal_image_frame(&bytes, STREAM, 23, scope_at)
            .unwrap()
            .unwrap();
        let scope = crate::records::feature::scope::DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:scope#23",
            crate::records::feature::scope::DesignFeatureKind::Decal,
            23,
        );
        let group = crate::records::topology::construction::DesignConstructionOperandGroup::try_from(
            crate::records::topology::construction::DesignConstructionOperandGroupDraft {
                id: "f3d:Design/BulkStream.dat:operand-group#24".into(),
                scope_record_index: 23,
                scope_reference_ordinal: 0,
                record_index: 24,
                byte_offset: 900,
                class_tag: crate::records::references::DesignClassTag::try_from("269".to_owned()).unwrap(),
                members: vec![crate::records::identity::Located { value: 100, offset: 926 }],
                lost_edge_references: Vec::new(),
                frame: crate::records::topology::construction::DesignConstructionOperandGroupFrame::try_from(
                    crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                        member_count_offset: 921,
                        auxiliary_records: Vec::new(),
                        auxiliary_paths: Vec::new(),
                        trailing_records: vec![crate::records::identity::Located { value: 200, offset: 943 }],
                        trailing_transforms: Vec::new(),
                        trailing_dual_transforms: Vec::new(),
                        trailing_flags: Vec::new(),
                        opaque_index: 1,
                        opaque_index_offset: 971,
                        opaque_scalar: 0.0,
                        opaque_scalar_offset: 975,
                        variant: false,
                        closing_flag: false,
                    },
                ).unwrap(),
                operand_role: crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    crate::records::topology::extrude_selection::DesignOperandRole::BODIES_A,
                ),
                role_offset: 953,
                paired_class_tag: crate::records::references::DesignClassTag::try_from("265".to_owned()).unwrap(),
                paired_byte_offset: 1024,
            },
        ).unwrap();
        let face = cadmpeg_ir::ids::FaceId::mint("test:model:face#1").unwrap();
        let operand = crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(
            crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
                id: "f3d:Design/BulkStream.dat:body-operand#100".into(),
                scope_record_index: 23,
                owner: crate::records::topology::body_recipe::DesignOperandOwner::Group {
                    group_record_index: 24,
                    group_member_ordinal: 0,
                },
                record_index: 100,
                byte_offset: 1000,
                class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 1056,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "11111111-2222-4333-8444-555555555555".to_owned(),
                )
                .unwrap(),
                context_id_offset: 1090,
                selector_tail: None,
                references: vec![
                    crate::records::topology::body_recipe::DesignBodyRecipeReference {
                        design_reference: 1,
                        design_reference_offset: 1025,
                        form: 1,
                        form_offset: 1033,
                        candidate_faces: vec![face.clone()],
                        preceding_candidate_faces: Vec::new(),
                        preceding_body_slots: Vec::new(),
                    },
                ],
                nested_record_index: 103,
                nested_record_index_offset: 1038,
                recipe_id: "f3d:Design/BulkStream.dat:recipe#1".into(),
                resolved_face_slot: None,
                resolved_body_state_id: None,
                resolved_body_slot: None,
                resolved_body_face_slots: Vec::new(),
                next_record_index: 104,
                next_byte_offset: 1200,
            },
        )
        .unwrap();
        let feature = || Feature {
            product_definition: None,
            id: crate::ids::neutral_feature_id(&scope),
            ordinal: 0,
            name: None,
            suppressed: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
                FeatureOperation::Sketch {
                    sketch: SketchFeatureBinding::Unresolved,
                },
            )),
            native_ref: Some(scope.id.clone()),
        };
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(b"PNG").unwrap();
        let archive = zip.finish().unwrap().into_inner();
        crate::test_support::zip_test::with_scan(&archive, |scan| {
            let assets = super::project_decal_images(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                std::slice::from_ref(&scope),
                std::slice::from_ref(&image),
                std::slice::from_ref(&group),
                std::slice::from_ref(&operand),
                &mut [feature()],
            )
            .unwrap();
            assert_eq!(assets.len(), 1);
            assert_eq!(assets[0].id, crate::ids::neutral_asset_id(ENTRY));
        });
    }
}
