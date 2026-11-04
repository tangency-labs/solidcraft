// SPDX-License-Identifier: Apache-2.0
//! Cross-record admission of native Pipe constructions.
//!
//! A Pipe scope is complete only when every scope reference is claimed exactly
//! once by a typed scalar owner, the settings record, a construction group, a
//! group member, or the optional point-selection pair.

use super::DesignPipeConstruction;
use crate::ids::design_segment;
use crate::native::F3dNative;
use crate::records::decal::DesignRecordHeader;
use crate::records::entity_header::{SegmentType, SegmentTypeData};
use crate::records::feature::extrude::DesignExtrudeOperation;
use crate::records::feature::scope::{DesignParameterScope, DesignScopePayload};
use crate::records::parameters::DesignParameterOwner;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use std::collections::{HashMap, HashSet};

pub(crate) const PIPE_GUID: &str = "73615646-8752-4EE6-AD02-0B90804B7CD7";
pub(crate) const PIPE_BASE: &str = "3FED63D9-CDC5-4334-A5C5-6C10271320D5";
pub(crate) const SCALAR_GUID: &str = "D91D429C-1BB5-4640-B49F-69AF14F463E9";
pub(crate) const SCALAR_BASE: &str = "B24D5130-8751-4272-8143-D230D8757873";
pub(crate) const SETTINGS_GUID: &str = "9414C87F-8EA3-465D-9F78-FF2FF5E9881F";
pub(crate) const SETTINGS_BASE: &str = "60F291CB-499B-4AB1-B0E5-340368671724";
pub(crate) const GROUP_GUID: &str = "2CA5A1CD-C99B-4C9B-91AF-57989148E841";
pub(crate) const GROUP_BASE: &str = "F8CD0BB3-2CE6-4236-8C1B-6D0638BF215F";
pub(crate) const BODY_GUID: &str = "9716F783-676D-42E0-93F9-EBF273E7C035";
pub(crate) const SELECTION_BASE: &str = "6144EA59-3ACA-4172-9F14-A6531E94D69B";
pub(crate) const PERSISTENT_GUID: &str = "5A1BF548-241F-46FD-9FB5-E4B05126EB9D";
pub(crate) const RECIPE_GUID: &str = "5662F619-2281-4AD9-93DC-1FCFDA1062E7";
pub(crate) const POINT_GUID: &str = "69EE2FA7-BCC7-449E-9CA9-976CEFDFED44";

/// Construction-group payload lengths from the opaque index to the paired header.
const GROUP_TAIL_LENGTHS: [u64; 4] = [47, 48, 51, 52];

fn has_type(data: &SegmentTypeData, guid: &str, base: &str, versions: &[u32]) -> bool {
    data.type_guid.as_str().eq_ignore_ascii_case(guid)
        && data
            .base_type_guid
            .value()
            .is_some_and(|value| value.as_str().eq_ignore_ascii_case(base))
        && versions.contains(&data.version)
}

pub(crate) fn is_pipe(data: &SegmentTypeData) -> bool {
    has_type(data, PIPE_GUID, PIPE_BASE, &[2])
}

pub(crate) fn is_scalar(data: &SegmentTypeData) -> bool {
    has_type(data, SCALAR_GUID, SCALAR_BASE, &[0])
}

pub(crate) fn is_point(data: &SegmentTypeData) -> bool {
    has_type(data, POINT_GUID, GROUP_GUID, &[2, 3])
}

fn is_recipe(data: &SegmentTypeData) -> bool {
    has_type(data, RECIPE_GUID, SELECTION_BASE, &[2, 3])
}

/// Whether a Pipe's operation has an assigned construction-group role sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PipeRoleAssessment {
    /// `NewBody`, `Join` or `Cut` groups follow their assigned role sequence.
    Assigned,
    /// Intersect keeps its generic group relations.
    UnassignedIntersect,
}

/// Insert a value under `key`, or mark the key ambiguous when it repeats.
fn insert_unique<'a, T>(
    map: &mut HashMap<(&'a str, u32), Option<&'a T>>,
    key: (&'a str, u32),
    value: &'a T,
) {
    map.entry(key)
        .and_modify(|slot| *slot = None)
        .or_insert(Some(value));
}

/// Native records a Pipe relation joins, keyed by Design segment and record index.
pub(crate) struct PipeIndex<'a> {
    types: HashMap<(&'a str, u32), Option<&'a SegmentType>>,
    classes: HashMap<(&'a str, u32), &'a SegmentType>,
    headers: HashMap<(&'a str, u32), Option<&'a DesignRecordHeader>>,
    owners: HashMap<(&'a str, u32), Option<&'a DesignParameterOwner>>,
    groups: HashMap<(&'a str, u32), Vec<&'a DesignConstructionOperandGroup>>,
}

impl<'a> PipeIndex<'a> {
    pub(crate) fn new(
        types: &'a [SegmentType],
        headers: &'a [DesignRecordHeader],
        owners: &'a [DesignParameterOwner],
        groups: &'a [DesignConstructionOperandGroup],
    ) -> Self {
        let mut index = Self {
            types: HashMap::new(),
            classes: HashMap::new(),
            headers: HashMap::new(),
            owners: HashMap::new(),
            groups: HashMap::new(),
        };
        // Dynamic class tags number each segment's type rows from 256 in
        // MetaStream order.
        let mut ordered = types
            .iter()
            .filter_map(|data| Some((design_segment(data.id())?, data)))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|(segment, data)| (*segment, data.byte_offset));
        let mut previous = None;
        let mut class = Some(256_u32);
        for (segment, data) in ordered {
            if previous != Some(segment) {
                class = Some(256);
                previous = Some(segment);
            }
            if let Some(current) = class {
                index.classes.insert((segment, current), data);
                class = current.checked_add(1);
            }
            for entity in data.entities.values() {
                if let Ok(entity) = u32::try_from(*entity) {
                    insert_unique(&mut index.types, (segment, entity), data);
                }
            }
        }
        for header in headers {
            if let Some(segment) = design_segment(&header.id) {
                insert_unique(&mut index.headers, (segment, header.record_index), header);
            }
        }
        for owner in owners {
            if let Some(segment) = design_segment(owner.id()) {
                insert_unique(&mut index.owners, (segment, owner.record_index()), owner);
            }
        }
        for group in groups {
            if let Some(segment) = design_segment(&group.id) {
                index
                    .groups
                    .entry((segment, group.scope_record_index))
                    .or_default()
                    .push(group);
            }
        }
        index
    }

    /// The unique parameter owner with this record index.
    pub(crate) fn owner(&self, segment: &str, id: u32) -> Option<&'a DesignParameterOwner> {
        self.owners.get(&(segment, id)).copied().flatten()
    }

    /// Construction groups owned by the scope with this record index.
    pub(crate) fn groups<'s>(
        &'s self,
        segment: &'s str,
        scope: u32,
    ) -> impl Iterator<Item = &'s DesignConstructionOperandGroup> + 's {
        self.groups
            .get(&(segment, scope))
            .into_iter()
            .flatten()
            .copied()
    }

    /// The unique type registration and header of a record whose class tag
    /// selects that same type row.
    fn typed(
        &self,
        segment: &str,
        id: u32,
    ) -> Option<(&'a SegmentTypeData, &'a DesignRecordHeader)> {
        let data = self.types.get(&(segment, id)).copied().flatten()?;
        let header = self.headers.get(&(segment, id)).copied().flatten()?;
        let class = header.class_tag.as_str().parse::<u32>().ok()?;
        let selected = self.classes.get(&(segment, class))?;
        std::ptr::eq(data, *selected).then_some((&**data, header))
    }

    /// Check the complete native relation of one Pipe scope.
    ///
    /// Returns `None` when any reference, type, owner, group, member, or point
    /// relation is missing, ambiguous, or out of place.
    pub(crate) fn check_relation(
        &self,
        scope: &DesignParameterScope,
        pipe: &DesignPipeConstruction,
    ) -> Option<PipeRoleAssessment> {
        let segment = design_segment(&scope.id)?;
        let (data, header) = self.typed(segment, scope.record_index)?;
        if !is_pipe(data)
            || header.byte_offset != scope.byte_offset()
            || header.class_tag != scope.class_tag
            || !pipe.scope_offsets_are_valid(scope.byte_offset(), scope.reference_count_offset())
        {
            return None;
        }
        let members = scope.reference_members();
        let mut references = HashSet::new();
        if !members.values().all(|id| references.insert(*id)) {
            return None;
        }
        let mut claimed = HashSet::new();
        let mut previous = None;
        for (ordinal, id) in (0_u32..).zip(pipe.record_indexes) {
            let (data, header) = self.typed(segment, id)?;
            let owner = self.owner(segment, id)?;
            let position = members.values().position(|value| *value == id)?;
            if !is_scalar(data)
                || owner.scope_record_index() != scope.record_index
                || owner.local_ordinal() != ordinal
                || owner.byte_offset() != header.byte_offset
                || owner.class_tag() != &header.class_tag
                || !matches!(owner.frame_length(), 103 | 104)
                || owner.byte_offset().checked_add(40) != Some(owner.evaluated_value_offset())
                || previous.is_some_and(|previous| position <= previous)
                || !claimed.insert(id)
            {
                return None;
            }
            previous = Some(position);
        }
        let mut settings = 0_usize;
        for id in members.values().copied() {
            let (data, _) = self.typed(segment, id)?;
            if has_type(data, SETTINGS_GUID, SETTINGS_BASE, &[1]) {
                settings += 1;
                if !claimed.insert(id) {
                    return None;
                }
            }
        }
        if settings != 1 {
            return None;
        }
        let mut groups = self.groups(segment, scope.record_index).collect::<Vec<_>>();
        groups.sort_by_key(|group| group.scope_reference_ordinal);
        for group in &groups {
            let (data, header) = self.typed(segment, group.record_index)?;
            let ordinal = usize::try_from(group.scope_reference_ordinal).ok()?;
            if !has_type(data, GROUP_GUID, GROUP_BASE, &[1])
                || group.byte_offset != header.byte_offset
                || group.class_tag != header.class_tag
                || group.members().is_empty()
                || group.extrude_role().is_some()
                || group.paired_byte_offset <= group.role_offset()
                || !GROUP_TAIL_LENGTHS.iter().any(|length| {
                    group.frame.opaque_index_offset().checked_add(*length)
                        == Some(group.paired_byte_offset)
                })
                || group
                    .byte_offset
                    .checked_add(21)
                    .is_none_or(|offset| group.frame.member_count_offset < offset)
                || members.values().nth(ordinal) != Some(&group.record_index)
                || !claimed.insert(group.record_index)
            {
                return None;
            }
            // The group's members follow it immediately in the scope table.
            let member_start = ordinal.checked_add(1)?;
            if members
                .len()
                .checked_sub(member_start)
                .is_none_or(|count| count < group.members().len())
            {
                return None;
            }
            let mut offset = group.frame.member_count_offset.checked_add(5);
            for (member, scope_member) in group
                .members()
                .iter()
                .zip(members.values().skip(member_start))
            {
                let (data, _) = self.typed(segment, member.value)?;
                let typed = match group.role() {
                    DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B => {
                        has_type(data, BODY_GUID, SELECTION_BASE, &[1])
                    }
                    DesignOperandRole::ROLE_0X5 => {
                        has_type(data, PERSISTENT_GUID, SELECTION_BASE, &[1]) || is_recipe(data)
                    }
                    _ => false,
                };
                if !typed
                    || member.value != *scope_member
                    || offset != Some(member.offset)
                    || member
                        .offset
                        .checked_add(10)
                        .is_none_or(|end| end > group.role_offset())
                    || !claimed.insert(member.value)
                {
                    return None;
                }
                offset = member.offset.checked_add(11);
            }
        }
        if let Some(selection) = &pipe.point_selection {
            let point = &selection.point_data;
            let (data, header) = self.typed(segment, point.point_record_index)?;
            let [input] = point.rule.inputs() else {
                return None;
            };
            let (input_type, _) = self.typed(segment, input.record_index())?;
            let input_gap = if data.version == 3 { 33 } else { 9 };
            if !is_point(data)
                || !is_recipe(input_type)
                || point.rule.reference_type() != 1
                || point.point_record_byte_offset != header.byte_offset
                || point
                    .point_record_byte_offset
                    .checked_add(42)
                    .is_none_or(|offset| point.position_offset < offset)
                || point.position_offset.checked_add(24) != Some(point.reference_type_offset)
                || point.reference_type_offset.checked_add(input_gap)
                    != Some(input.reference_offset)
                || !claimed.insert(point.point_record_index)
                || !claimed.insert(input.record_index())
            {
                return None;
            }
        }
        if claimed != references {
            return None;
        }
        let role = |group: &DesignConstructionOperandGroup| group.role();
        let assigned = match pipe.operation {
            DesignExtrudeOperation::NewBody => {
                matches!(groups.as_slice(), [path] if role(path) == DesignOperandRole::ROLE_0X5)
            }
            DesignExtrudeOperation::Join | DesignExtrudeOperation::Cut => matches!(
                groups.as_slice(),
                [body, path] if matches!(role(body), DesignOperandRole::BODIES_A | DesignOperandRole::BODIES_B)
                    && role(path) == DesignOperandRole::ROLE_0X5
            ),
            DesignExtrudeOperation::Intersect => {
                return Some(PipeRoleAssessment::UnassignedIntersect)
            }
        };
        assigned.then_some(PipeRoleAssessment::Assigned)
    }
}

/// Reject a native document whose Pipe construction lacks its complete relation.
pub(crate) fn admit_native(native: &F3dNative) -> Result<(), cadmpeg_ir::NativeConvertError> {
    if !native
        .design_parameter_scopes
        .iter()
        .any(|scope| matches!(scope.payload(), DesignScopePayload::Pipe(Some(_))))
    {
        return Ok(());
    }
    let index = PipeIndex::new(
        &native.design_types,
        &native.design_record_headers,
        &native.design_parameter_owners,
        &native.design_construction_operand_groups,
    );
    for scope in &native.design_parameter_scopes {
        if let DesignScopePayload::Pipe(Some(pipe)) = scope.payload() {
            if index.check_relation(scope, pipe).is_none() {
                return Err(cadmpeg_ir::NativeConvertError::InvalidCollection(format!(
                    "invalid F3D Pipe native relation for {}",
                    scope.id
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
