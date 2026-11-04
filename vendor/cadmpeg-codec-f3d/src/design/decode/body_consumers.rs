// SPDX-License-Identifier: Apache-2.0
//! Bodies that a `Combine` or Remove scope names as consumed.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;

use crate::bytes::take_reference;
use crate::design::decode::text::fixed_guid_end;
use crate::layout::body_consumer_browser_node_prefix as node_prefix;
use crate::metastream::{primary_record_frames, MetaStream, PrimaryRecordFrame};

pub(crate) const COMBINE_TYPE_GUID: &str = "2A94257F-2020-4B19-9A68-103A2672F1B7";
pub(crate) const REMOVE_BODY_TYPE_GUID: &str = "4A782808-FBDD-4351-A10A-F7AE5693D330";
const LOCAL_REFERENCE_LEN: usize = 11;

/// Design entity suffixes of the bodies whose hidden browser nodes a consuming
/// scope selects.
pub(super) fn consumed_body_entities(
    root: View<'_>,
    meta: &MetaStream,
) -> Result<HashSet<u64>, CodecError> {
    let consumers = entities_of(meta, |guid| {
        [COMBINE_TYPE_GUID, REMOVE_BODY_TYPE_GUID]
            .iter()
            .any(|consumer| consumer.eq_ignore_ascii_case(guid))
    });
    if consumers.is_empty() {
        return Ok(HashSet::new());
    }
    let nodes = entities_of(meta, |guid| {
        crate::design::presentation::BROWSER_NODE_TYPE_GUID.eq_ignore_ascii_case(guid)
    });
    let frames: HashMap<_, _> = primary_record_frames(meta, root.remaining())?
        .into_iter()
        .map(|frame| (frame.entity_id, frame))
        .collect();
    let bytes = root.window();
    let mut consumed = HashSet::new();
    super::body_reference_tails::for_each_selection(root, meta, &frames, |frame, selected| {
        if !owner_is(bytes, frame, &consumers) {
            return;
        }
        for node in selected {
            if !nodes.contains(node) {
                continue;
            }
            if let Some(entity) = frames
                .get(node)
                .and_then(|node| hidden_node_entity(bytes, *node))
            {
                consumed.insert(entity);
            }
        }
    });
    Ok(consumed)
}

fn entities_of(meta: &MetaStream, selected: impl Fn(&str) -> bool) -> HashSet<u64> {
    meta.types
        .iter()
        .filter(|kind| selected(kind.type_guid.as_str()))
        .flat_map(|kind| kind.entities.values().copied())
        .collect()
}

/// A selection record ends with a local reference to the scope that owns it.
fn owner_is(bytes: &[u8], frame: PrimaryRecordFrame, owners: &HashSet<u64>) -> bool {
    let Some(mut cursor) = frame.member_end.checked_sub(LOCAL_REFERENCE_LEN) else {
        return false;
    };
    take_reference(bytes, &mut cursor)
        .and_then(crate::bytes::Reference::into_local)
        .is_some_and(|(owner, _)| owners.contains(&owner) && cursor == frame.member_end)
}

fn hidden_node_entity(bytes: &[u8], frame: PrimaryRecordFrame) -> Option<u64> {
    let record = bytes.get(frame.start..frame.member_end)?;
    let guid_end = fixed_guid_end(record, node_prefix::GUID_CODE_UNIT_COUNT)?;
    let mut cursor = guid_end.checked_add(2)?;
    if !matches!(record.get(guid_end..cursor)?, [1, 0 | 1]) {
        return None;
    }
    Some(take_reference(record, &mut cursor)?.into_local()?.0)
}
