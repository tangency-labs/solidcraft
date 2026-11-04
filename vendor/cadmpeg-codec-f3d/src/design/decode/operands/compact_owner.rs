// SPDX-License-Identifier: Apache-2.0
//! Source namespaces selected by compact construction owner carriers.
use crate::container::ContainerScan;
use crate::design::decode::meta::{
    design_primary_frames, metadata_for_bulk_stream, DesignPrimaryFrame,
};
use crate::ids::native_stream;
use crate::layout::{compact_owner_body_carrier as carrier, compact_owner_body_wrapper as wrapper};
use crate::records::bodies::DesignBodyBinding;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandIdentity,
};
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::HashMap;

fn typed(frame: &DesignPrimaryFrame<'_>, guid: &str, base: &str, version: u32) -> bool {
    let kind = frame.design_type;
    kind.module == "EntityTracking"
        && kind.version == version
        && kind.type_guid.as_str().eq_ignore_ascii_case(guid)
        && kind
            .base_type_guid
            .value()
            .is_some_and(|value| value.as_str().eq_ignore_ascii_case(base))
}

fn suffix(
    bytes: &[u8],
    owner: &DesignPrimaryFrame<'_>,
    body: &DesignPrimaryFrame<'_>,
) -> Option<u64> {
    if owner.end.checked_sub(owner.start)? != wrapper::LEN
        || body.end.checked_sub(body.start)? != carrier::LEN
        || owner.end != body.start
        || owner.entity_id.checked_add(1)? != body.entity_id
        || !typed(
            owner,
            "CB4F4CC2-9D6F-48BA-9B1F-20C1992C9C9C",
            "5C069ACB-169F-4555-9792-815B4671DDA0",
            2,
        )
        || !typed(
            body,
            "90055C05-546C-4EE7-B3C9-3DD922AD0C9C",
            "70F5E801-1E8E-436E-8A09-36487F8A2859",
            0,
        )
        || bytes.get(owner.start + wrapper::ZERO_PREFIX..owner.start + wrapper::REFERENCE_MARKER)?
            != [0; 10]
        || bytes.get(owner.start + wrapper::REFERENCE_MARKER) != Some(&1)
        || View::u64_le_at(bytes, owner.start + wrapper::CARRIER_RECORD_INDEX)? != body.entity_id
        || bytes.get(owner.start + wrapper::ZERO_SUFFIX..owner.end)? != [0; 3]
        || bytes.get(body.start + carrier::ZERO_PREFIX..body.start + carrier::BODY_SUFFIX)?
            != [0; 10]
    {
        return None;
    }
    View::u64_le_at(bytes, body.start + carrier::BODY_SUFFIX)
}

pub(crate) fn source_selectors(
    scan: &ContainerScan<'_>,
    groups: &[DesignConstructionOperandGroup],
    identities: &[DesignConstructionOperandIdentity],
    bindings: &[DesignBodyBinding],
) -> Result<HashMap<String, Vec<String>>, CodecError> {
    let mut out = HashMap::<String, Vec<String>>::new();
    for entry in scan.entries.iter().filter(|entry| {
        scan.is_design_stream(entry, cadmpeg_core::container::ContainerRole::Bulkstream)
    }) {
        let Some(meta) = metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let frames = design_primary_frames(bytes, &meta)?;
        for identity in identities
            .iter()
            .filter(|identity| native_stream(&identity.id) == Some(entry.name.as_str()))
        {
            let Some((index, owner)) = frames.iter().enumerate().find(|(_, frame)| {
                frame.entity_id == u64::from(identity.following_record_index())
                    && u64::try_from(frame.start).ok() == Some(identity.following_byte_offset())
            }) else {
                continue;
            };
            let Some(body) = frames.get(index + 1) else {
                continue;
            };
            let Some(body_suffix) = suffix(bytes, owner, body) else {
                continue;
            };
            for group in groups.iter().filter(|group| {
                native_stream(&group.id) == Some(entry.name.as_str())
                    && group.record_index == identity.group_record_index
            }) {
                for binding in bindings.iter().filter(|binding| {
                    native_stream(binding.id()) == Some(entry.name.as_str())
                        && binding.entity_suffix == body_suffix
                }) {
                    if let Some(source) = binding.blob_name().strip_prefix("BREP.") {
                        out.entry(group.id.clone())
                            .or_default()
                            .push(source.to_owned());
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::suffix;
    use crate::design::decode::meta::DesignPrimaryFrame;
    use crate::records::entity_header::{BaseTypeGuid, SegmentTypeData};
    use crate::records::identity::ReferenceRun;

    fn kind(guid: &str, base: &str, version: u32) -> SegmentTypeData {
        SegmentTypeData {
            byte_offset: 0,
            type_guid: guid.to_owned().try_into().unwrap(),
            type_guid_offset: 0,
            base_type_guid: BaseTypeGuid::Guid {
                value: base.to_owned().try_into().unwrap(),
                offset: 0,
            },
            version,
            version_offset: 0,
            module: "EntityTracking".into(),
            entities: ReferenceRun::unlocated(Vec::new()),
        }
    }

    #[test]
    fn compact_owner_body_requires_adjacent_typed_frames_and_matching_reference() {
        fn body(kind: &SegmentTypeData) -> DesignPrimaryFrame<'_> {
            DesignPrimaryFrame {
                entity_id: 41,
                class_tag: "531".to_owned().try_into().unwrap(),
                start: 33,
                member_end: 62,
                end: 62,
                design_type: kind,
            }
        }
        let owner_type = kind(
            "CB4F4CC2-9D6F-48BA-9B1F-20C1992C9C9C",
            "5C069ACB-169F-4555-9792-815B4671DDA0",
            2,
        );
        let mut body_type = kind(
            "90055C05-546C-4EE7-B3C9-3DD922AD0C9C",
            "70F5E801-1E8E-436E-8A09-36487F8A2859",
            0,
        );
        let owner = DesignPrimaryFrame {
            entity_id: 40,
            class_tag: "264".to_owned().try_into().unwrap(),
            start: 0,
            member_end: 33,
            end: 33,
            design_type: &owner_type,
        };
        let mut bytes = vec![0; 62];
        bytes[21] = 1;
        bytes[22..30].copy_from_slice(&41u64.to_le_bytes());
        bytes[54..62].copy_from_slice(&1234u64.to_le_bytes());
        assert_eq!(suffix(&bytes, &owner, &body(&body_type)), Some(1234));
        bytes[22..30].copy_from_slice(&42u64.to_le_bytes());
        assert_eq!(suffix(&bytes, &owner, &body(&body_type)), None);
        bytes[22..30].copy_from_slice(&41u64.to_le_bytes());
        bytes[53] = 1;
        assert_eq!(suffix(&bytes, &owner, &body(&body_type)), None);
        bytes[53] = 0;
        body_type.version = 2;
        assert_eq!(suffix(&bytes, &owner, &body(&body_type)), None);
        body_type.version = 0;
        body_type.module = "Fusion".into();
        assert_eq!(suffix(&bytes, &owner, &body(&body_type)), None);
        body_type.module = "EntityTracking".into();
        let mut separated = body(&body_type);
        separated.start += 1;
        separated.end += 1;
        assert_eq!(suffix(&bytes, &owner, &separated), None);
    }
}
