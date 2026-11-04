// SPDX-License-Identifier: Apache-2.0
//! Registration admission for construction tracking wrappers and carriers.
use crate::metastream::MetaStream;
use crate::records::entity_header::SegmentTypeData;
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::DesignConstructionTrackingPath;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Wrapper,
    Carrier,
    TrailingTransform,
}

pub(super) struct Registry {
    meta: Arc<MetaStream>,
    kinds: HashMap<usize, Kind>,
    entities: HashSet<(usize, u64)>,
}

fn registration(kind: &SegmentTypeData) -> Option<Kind> {
    if kind.module != "EntityTracking" {
        return None;
    }
    let base = kind.base_type_guid.value()?.as_str();
    if kind.version == 0
        && kind
            .type_guid
            .as_str()
            .eq_ignore_ascii_case("335CC7BD-BBE5-40AA-9E8C-5EC99D427A3A")
        && base.eq_ignore_ascii_case("5C069ACB-169F-4555-9792-815B4671DDA0")
    {
        Some(Kind::Wrapper)
    } else if kind.version == 2
        && kind
            .type_guid
            .as_str()
            .eq_ignore_ascii_case("2DEE34BE-FA70-4D54-979C-164D86FDA786")
        && base.eq_ignore_ascii_case("70F5E801-1E8E-436E-8A09-36487F8A2859")
    {
        Some(Kind::Carrier)
    } else if kind.version == 0
        && kind
            .type_guid
            .as_str()
            .eq_ignore_ascii_case("2DF7DA30-A4E4-4260-AFD1-D9C7154CB44B")
        && base.eq_ignore_ascii_case("872045B4-F653-4691-9752-ABC7684834BF")
    {
        Some(Kind::TrailingTransform)
    } else {
        None
    }
}

impl Registry {
    pub(super) fn new(meta: Arc<MetaStream>) -> Self {
        let mut kinds = HashMap::new();
        let mut entities = HashSet::new();
        for (ordinal, row) in meta.types.iter().enumerate() {
            let Some(kind) = registration(row) else {
                continue;
            };
            kinds.insert(ordinal, kind);
            entities.extend(row.entities.values().map(|entity| (ordinal, *entity)));
        }
        Self {
            meta,
            kinds,
            entities,
        }
    }

    fn header(&self, class: &DesignClassTag, entity: u32, offset: u64, kind: Kind) -> bool {
        let Some(ordinal) = class.dynamic_ordinal() else {
            return false;
        };
        self.kinds.get(&ordinal) == Some(&kind)
            && self.entities.contains(&(ordinal, u64::from(entity)))
            && self
                .meta
                .records
                .binary_search_by_key(&offset, |record| record.bulk_offset)
                .is_ok_and(|index| self.meta.records[index].entity_id == u64::from(entity))
    }

    pub(super) fn supports_transform(
        &self,
        transform: &crate::records::topology::construction::DesignConstructionOperandTransform,
    ) -> bool {
        if !self.header(
            &transform.class_tag,
            transform.record_index(),
            transform.byte_offset(),
            Kind::TrailingTransform,
        ) {
            return false;
        }
        let Ok(index) = self
            .meta
            .records
            .binary_search_by_key(&transform.byte_offset(), |record| record.bulk_offset)
        else {
            return false;
        };
        let Some(next) = self.meta.records.get(index + 1) else {
            return false;
        };
        let Some(ordinal) = transform.following_class_tag.dynamic_ordinal() else {
            return false;
        };
        next.bulk_offset == transform.following_byte_offset()
            && next.entity_id == u64::from(transform.following_record_index())
            && self.meta.types.get(ordinal).is_some_and(|row| {
                row.entities
                    .values()
                    .any(|entity| *entity == next.entity_id)
            })
    }

    pub(super) fn supports(&self, path: &DesignConstructionTrackingPath) -> bool {
        self.header(
            &path.wrapper_class_tag,
            path.wrapper_record_index(),
            path.wrapper_byte_offset(),
            Kind::Wrapper,
        ) && self.header(
            &path.carrier_class_tag,
            path.carrier_record_index(),
            path.carrier_byte_offset(),
            Kind::Carrier,
        )
    }
}

#[cfg(test)]
mod tests;
