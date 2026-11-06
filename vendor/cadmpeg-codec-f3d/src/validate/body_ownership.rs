// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::records::bodies::DesignBodyBinding;
use crate::records::entity_header::SegmentTypeData;

pub(super) struct OwnerTypes<'a> {
    registered: BTreeMap<(&'a str, u64), Option<&'a SegmentTypeData>>,
}

impl<'a> OwnerTypes<'a> {
    pub(super) fn new(
        native: &'a crate::native::F3dNative,
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<Self, cadmpeg_core::CodecError> {
        let mut registered: BTreeMap<(&str, u64), Option<&SegmentTypeData>> = BTreeMap::new();
        for data in native
            .design_types
            .iter()
            .filter(|data| crate::design::decode::component_bodies::ownership_type(data))
        {
            let Some(segment) = crate::ids::design_segment(data.id()) else {
                continue;
            };
            for entity in data.entities.values() {
                if !registered.contains_key(&(segment, *entity)) {
                    decode.retain_bytes(
                        cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                            (&str, u64),
                            Option<&SegmentTypeData>,
                        )>()),
                        "validate Fusion component ownership type index",
                    )?;
                }
                registered
                    .entry((segment, *entity))
                    .and_modify(|entry| *entry = None)
                    .or_insert(Some(data));
            }
        }
        Ok(Self { registered })
    }

    pub(super) fn matches(&self, binding: &DesignBodyBinding) -> bool {
        let Some(owner) = &binding.component_owner else {
            return true;
        };
        if binding.component_owner_unresolved {
            return false;
        }
        let Some(segment) = crate::ids::design_segment(binding.id()) else {
            return false;
        };
        let lookup = |entity| self.registered.get(&(segment, entity)).copied().flatten();
        crate::design::decode::component_bodies::valid_owner(owner, binding.entity_suffix, lookup)
    }
}
