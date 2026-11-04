// SPDX-License-Identifier: Apache-2.0
//! Join live Body-container membership to component definitions.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::records::body_ownership::{
    membership, terminal_membership, BodyOwnership, CONTAINER_BASE_GUID, CONTAINER_GUID,
};
use crate::records::entity_header::SegmentTypeData;

pub(crate) enum OwnerResolution {
    Admitted(BodyOwnership),
    Unadmitted,
    Conflicting,
}

impl OwnerResolution {
    pub(crate) fn admitted(&self) -> Option<&BodyOwnership> {
        match self {
            Self::Admitted(owner) => Some(owner),
            Self::Unadmitted | Self::Conflicting => None,
        }
    }

    fn merge(&mut self, incoming: Self, body: u64) {
        let Self::Admitted(owner) = incoming else {
            return;
        };
        match self {
            Self::Unadmitted => *self = Self::Admitted(owner),
            Self::Admitted(previous)
                if previous.component_for(body) != owner.component_for(body) =>
            {
                *self = Self::Conflicting;
            }
            Self::Admitted(_) | Self::Conflicting => {}
        }
    }
}

pub(crate) fn ownership_type(data: &SegmentTypeData) -> bool {
    [
        CONTAINER_GUID,
        crate::records::body_ownership::COMPONENT_GUID,
        crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
    ]
    .iter()
    .any(|guid| data.type_guid.as_str().eq_ignore_ascii_case(guid))
}

pub(crate) fn supported_container(data: &SegmentTypeData) -> bool {
    data.module == "Body"
        && data.version == 4
        && data.type_guid.as_str().eq_ignore_ascii_case(CONTAINER_GUID)
        && data
            .base_type_guid
            .value()
            .is_some_and(|base| base.as_str().eq_ignore_ascii_case(CONTAINER_BASE_GUID))
}

pub(crate) fn valid_owner<'a>(
    owner: &BodyOwnership,
    body: u64,
    lookup: impl Fn(u64) -> Option<&'a SegmentTypeData>,
) -> bool {
    let Some(component) = owner.component_for(body) else {
        return false;
    };
    lookup(owner.container_record_index).is_some_and(supported_container)
        && lookup(component).is_some_and(|data| {
            data.type_guid
                .as_str()
                .eq_ignore_ascii_case(crate::records::body_ownership::COMPONENT_GUID)
        })
        && lookup(body).is_some_and(|data| {
            data.type_guid
                .as_str()
                .eq_ignore_ascii_case(crate::design::presentation::BODY_PRESENTATION_TYPE_GUID)
        })
}

/// Body-container memberships of one Design stream.
pub(crate) struct BodyMemberships {
    /// Owner resolution of each listed Body entity.
    pub(crate) owners: BTreeMap<u64, OwnerResolution>,
    /// Whether the stream registers a Body container and every registered
    /// container has a primary frame with an admitted membership, so an entity
    /// absent from `owners` is absent from every list.
    pub(crate) complete: bool,
}

pub(crate) fn owners(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<BodyMemberships, CodecError> {
    let mut result: BTreeMap<u64, OwnerResolution> = BTreeMap::new();
    let registered_containers = meta
        .types
        .iter()
        .filter(|data| data.type_guid.as_str().eq_ignore_ascii_case(CONTAINER_GUID))
        .map(|data| data.entities.len())
        .sum::<usize>();
    let mut admitted_containers = 0_usize;
    let mut complete = true;
    let mut registered: BTreeMap<u64, Option<&SegmentTypeData>> = BTreeMap::new();
    for data in meta.types.iter().filter(|data| ownership_type(data)) {
        for entity in data.entities.values() {
            if !registered.contains_key(entity) {
                ctx.retain_bytes(
                    cadmpeg_core::decode::u64_from_index(std::mem::size_of::<(
                        u64,
                        Option<&SegmentTypeData>,
                    )>()),
                    "Fusion component ownership type index",
                )?;
            }
            registered
                .entry(*entity)
                .and_modify(|entry| *entry = None)
                .or_insert(Some(data));
        }
    }
    let lookup = |entity| registered.get(&entity).copied().flatten();
    for frame in super::meta::design_primary_frames(bytes, meta)? {
        if !supported_container(frame.design_type) {
            continue;
        }
        let record = &bytes[frame.start..frame.end];
        let Some(start) = terminal_membership(record) else {
            complete = false;
            continue;
        };
        let tail = &record[start..];
        let Some(join) = membership(tail) else {
            complete = false;
            continue;
        };
        admitted_containers += 1;
        for (ordinal, body) in join.bodies().enumerate() {
            let Some(owner) = join.proof(
                frame.entity_id,
                cadmpeg_core::decode::u64_from_index(frame.start + start),
                ordinal,
            ) else {
                complete = false;
                continue;
            };
            let incoming = if valid_owner(&owner, body, lookup)
                && owner.component_for(body) == Some(join.component)
            {
                OwnerResolution::Admitted(owner)
            } else {
                complete = false;
                OwnerResolution::Unadmitted
            };
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<OwnerResolution>()),
                "Fusion component body membership",
            )?;
            match result.entry(body) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(incoming);
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().merge(incoming, body);
                }
            }
        }
    }
    Ok(BodyMemberships {
        owners: result,
        complete: complete
            && admitted_containers != 0
            && admitted_containers == registered_containers,
    })
}
