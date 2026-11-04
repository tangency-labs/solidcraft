// SPDX-License-Identifier: Apache-2.0
//! Assemble field grammar for registered scope versions.

use super::assembly_operand_paths::{exact_assembly_operand_paths, StreamTypes};
use super::shared_frames::{exact_indexed_header_at, rigid_transform_at};
use crate::bytes::{local_reference_target, lp_ascii_filtered_view};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::ids::native_stream;
use crate::layout::assembly_version_five_scope as five;
use crate::layout::assembly_version_three_prefix as prefix;
use crate::records::feature::assembly::{
    DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyOperandFrame,
    DesignAssemblyOperandQualifier, DesignJointFrameSense,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_core::CodecError;

pub(crate) const TYPE_GUID: &str = "8DFDD543-F823-43AA-BDAF-C638E023F0A0";
/// Type GUID of the referenced joint record.
const JOINT_TYPE_GUID: &str = "7F5A0426-1D2A-4DC4-9F06-58631F69D2F5";

pub(crate) fn registered(scope: &DesignParameterScope, types: &StreamTypes<'_>) -> bool {
    scope.kind() == DesignFeatureKind::Assemble
        && types
            .get(&u64::from(scope.record_index))
            .is_some_and(|(guid, version)| {
                matches!(version, 3 | 5) && guid.eq_ignore_ascii_case(TYPE_GUID)
            })
}

pub(crate) fn relation_only(
    scope: &DesignParameterScope,
    alignment: &DesignAssemblyAlignment,
    types: &StreamTypes<'_>,
) -> bool {
    let legacy_five = scope.frame_length() == u64_from_index(five::LEN)
        && alignment.operand_frames().is_some_and(|frames| {
            frames
                .iter()
                .zip([
                    (five::FIRST_REFERENCE, five::FIRST_MATRIX),
                    (five::SECOND_REFERENCE, five::SECOND_MATRIX),
                ])
                .all(|(frame, (reference, matrix))| {
                    frame.reference_offset.checked_sub(scope.byte_offset())
                        == Some(u64_from_index(reference + 1))
                        && frame.transform_offset.checked_sub(scope.byte_offset())
                            == Some(u64_from_index(matrix))
                })
        });
    registered(scope, types)
        && (types
            .get(&u64::from(scope.record_index))
            .is_some_and(|(_, version)| *version == 3)
            || !legacy_five)
}

#[derive(Clone, Copy)]
struct VersionLayout {
    joint_versions: &'static [u32],
    prologue_zeros: usize,
    frame_zeros: usize,
    frame_guid: bool,
    angle_zeros: usize,
    tail_reference: bool,
    tail_zeros: usize,
}

impl VersionLayout {
    fn for_scope(scope: &DesignParameterScope, types: &StreamTypes<'_>) -> Option<Self> {
        if !registered(scope, types) {
            return None;
        }
        Some(match types.get(&u64::from(scope.record_index))?.1 {
            3 => Self {
                joint_versions: &[3, 4],
                prologue_zeros: 0,
                frame_zeros: 0,
                frame_guid: false,
                angle_zeros: 0,
                tail_reference: false,
                tail_zeros: 0,
            },
            5 => Self {
                joint_versions: &[6],
                prologue_zeros: five::ZERO_RUN_26_VALUE.len() - 2,
                frame_zeros: five::FIRST_FRAME_ZEROS_VALUE.len(),
                frame_guid: true,
                angle_zeros: five::ANGLE_ZEROS_VALUE.len(),
                tail_reference: true,
                tail_zeros: five::TAIL_SEPARATOR_VALUE.len(),
            },
            _ => return None,
        })
    }

    fn extra_span(self, stride: usize) -> usize {
        self.prologue_zeros
            + 2 * (self.frame_zeros + usize::from(self.frame_guid) * 76)
            + self.angle_zeros
            + usize::from(self.tail_reference) * stride
            + self.tail_zeros
    }
}

fn version_five(scope: &DesignParameterScope, types: &StreamTypes<'_>) -> bool {
    types
        .get(&u64::from(scope.record_index))
        .is_some_and(|(_, version)| *version == 5)
}

/// Whether a scope's joint binds its feature only after archive merge, which
/// can still refuse a joint whose occurrence path does not complete.
pub(crate) fn joint_binds_after_merge(
    scope: &DesignParameterScope,
    types: &StreamTypes<'_>,
) -> bool {
    scope.assembly_alignment().is_some_and(|alignment| {
        alignment.frame_sense.is_some() && relation_only(scope, alignment, types)
    })
}

pub(crate) struct ScopeGrammar {
    pub(crate) frames: [DesignAssemblyOperandFrame; 2],
    pub(crate) owners: [u32; 4],
    pub(crate) locator_offsets: [usize; 2],
    pub(crate) stride: usize,
    pub(crate) facing: DesignJointFrameSense,
    pub(crate) joint: u32,
    pub(crate) tail_state: [u8; 2],
}

struct Fields<'a, 't> {
    view: View<'a>,
    bytes: &'a [u8],
    types: &'t StreamTypes<'t>,
    stride: usize,
    layout: VersionLayout,
}

impl Fields<'_, '_> {
    fn zero(&mut self, count: usize) -> Option<()> {
        self.view
            .take(count)?
            .iter()
            .all(|byte| *byte == 0)
            .then_some(())
    }

    fn reference(&mut self) -> Option<u32> {
        let at = self.view.position();
        let target = local_reference_target(
            self.bytes,
            at,
            self.stride == crate::bytes::TYPED_LOCAL_REFERENCE_LEN,
        )?;
        if target == 0 {
            return None;
        }
        if self.stride == crate::bytes::TYPED_LOCAL_REFERENCE_LEN {
            let (guid, _) =
                lp_ascii_filtered_view(self.bytes, at.checked_add(9)?, 36..=36, u8::is_ascii)?;
            let (expected, _) = self.types.get(&u64::from(target))?;
            if !guid.eq_ignore_ascii_case(expected) {
                return None;
            }
        }
        self.view.take(self.stride)?;
        Some(target)
    }

    fn guid(&mut self) -> Option<()> {
        let (guid, after) =
            super::super::text::fixed_relaxed_guid_text(self.bytes, self.view.position())
                .ok()??;
        if guid.as_str().len() != 36 {
            return None;
        }
        self.view.seek(after)?;
        Some(())
    }

    fn frame(&mut self) -> Option<DesignAssemblyOperandFrame> {
        let reference_offset = u64_from_index(self.view.position().checked_add(1)?);
        let reference_record_index = self.reference()?;
        self.zero(self.layout.frame_zeros)?;
        let flag_at = self.view.position();
        let identity = self.view.u8()?;
        let (transform, at) = match identity {
            0 => {
                let at = self.view.position();
                let transform = rigid_transform_at(self.bytes, at)?;
                self.view.take(128)?;
                (transform, at)
            }
            1 => (
                std::array::from_fn(|i| std::array::from_fn(|j| f64::from(u8::from(i == j))))
                    .try_into()
                    .ok()?,
                flag_at,
            ),
            _ => return None,
        };
        if self.layout.frame_guid {
            self.guid()?;
        }
        Some(DesignAssemblyOperandFrame {
            reference_record_index,
            reference_offset,
            transform,
            transform_offset: u64_from_index(at),
        })
    }
}

pub(crate) fn parse(
    bytes: &[u8],
    scope: &DesignParameterScope,
    types: &StreamTypes<'_>,
) -> Option<ScopeGrammar> {
    let layout = VersionLayout::for_scope(scope, types)?;
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = usize::try_from(scope.paired_byte_offset()).ok()?;
    if exact_indexed_header_at(bytes, start, scope.record_index)? != scope.class_tag.as_str()
        || exact_indexed_header_at(bytes, end, scope.record_index)?
            != scope.paired_class_tag.as_str()
        || end.checked_sub(start)? != usize::try_from(scope.frame_length()).ok()?
    {
        return None;
    }
    let mut view = View::over_retained(bytes).child(start, end)?;
    view.take(crate::layout::indexed_design_record_header::LEN)?;
    if view.take(prefix::ZERO_RUN_VALUE.len())? != prefix::ZERO_RUN_VALUE {
        return None;
    }
    let expanded = view.u8()?;
    match expanded {
        0 => (),
        1 => {
            if view.take(4)? != [0; 4] {
                return None;
            }
        }
        _ => return None,
    }
    let facing = match view.u8()? {
        0 => DesignJointFrameSense::Aligned,
        1 => DesignJointFrameSense::Opposed,
        _ => return None,
    };
    if view.take(2)? != [0; 2] {
        return None;
    }
    view.take(layout.prologue_zeros)?
        .iter()
        .all(|byte| *byte == 0)
        .then_some(())?;
    let stride = if View::u32_le_at(bytes, view.position().checked_add(9)?) == Some(36) {
        crate::bytes::TYPED_LOCAL_REFERENCE_LEN
    } else {
        11
    };
    if layout.frame_guid && stride != 11 {
        return None;
    }
    let mut fields = Fields {
        view,
        bytes,
        types,
        stride,
        layout,
    };
    let frames = [fields.frame()?, fields.frame()?];
    fields.zero(4)?;
    let angle = fields.reference()?;
    fields.zero(layout.angle_zeros)?;
    let owners = [
        angle,
        fields.reference()?,
        fields.reference()?,
        fields.reference()?,
    ];
    fields.zero(6)?;
    if fields.view.u32_le()? != 2 {
        return None;
    }
    let first = fields.view.position().checked_sub(start)?;
    fields.reference()?;
    let second = fields.view.position().checked_sub(start)?;
    fields.reference()?;
    let joint = fields.reference()?;
    if !types.get(&u64::from(joint)).is_some_and(|(guid, version)| {
        guid.eq_ignore_ascii_case(JOINT_TYPE_GUID) && layout.joint_versions.contains(version)
    }) {
        return None;
    }
    fields.guid()?;
    fields.zero(3)?;
    if u64_from_index(fields.view.position()) != scope.reference_count_offset() {
        return None;
    }
    let count = fields.view.u32_le()?;
    fields.view.counted(u64::from(count), stride)?;
    if usize::try_from(count).ok()? != scope.reference_members().len() {
        return None;
    }
    for (expected, offset) in scope
        .reference_members()
        .values()
        .zip(scope.reference_members().offsets())
    {
        if u64_from_index(fields.view.position().checked_add(1)?) != *offset
            || fields.reference()? != *expected
        {
            return None;
        }
    }
    fields.view.u32_le()?;
    let kind = b"\x08\x00\x00\x00A\x00s\x00s\x00e\x00m\x00b\x00l\x00e\x00";
    if fields.view.take(kind.len())? != kind {
        return None;
    }
    if u64_from_index(fields.view.position()) != scope.feature_ordinal_offset()
        || fields.view.u32_le()? != scope.feature_ordinal.get()
    {
        return None;
    }
    fields.zero(15)?;
    fields.reference()?;
    if layout.tail_reference {
        fields.reference()?;
    }
    let tail = fields.view;
    let mut separators = 0;
    let mut tail_state = [0; 2];
    for separator in 0..=usize::from(!layout.tail_reference) {
        fields.view = tail;
        let candidate = (|| {
            fields.zero(separator)?;
            fields.view.u32_le()?;
            fields.view.u32_le()?;
            if !fields.view.f64_le()?.is_finite() {
                return None;
            }
            fields.view.u32_le()?;
            fields.reference()?;
            let state = [fields.view.u8()?, fields.view.u8()?];
            if state.iter().any(|value| *value > 1) {
                return None;
            }
            fields.zero(layout.tail_zeros)?;
            fields.reference()?;
            fields.zero(2)?;
            let matrices = frames
                .iter()
                .filter(|frame| {
                    frame.transform_offset
                        == frame.reference_offset + u64_from_index(stride + layout.frame_zeros)
                })
                .count();
            let span = 190_usize
                .checked_add((usize::try_from(count).ok()?.checked_add(12)?).checked_mul(stride)?)?
                .checked_add(128 * matrices)?
                .checked_add(4 * usize::from(expanded))?
                .checked_add(separator)?
                .checked_add(layout.extra_span(stride))?;
            (fields.view.position() == end && span == end.checked_sub(start)?).then_some(state)
        })();
        if let Some(state) = candidate {
            separators += 1;
            tail_state = state;
        }
    }
    (separators == 1).then_some(ScopeGrammar {
        frames,
        owners,
        locator_offsets: [first, second],
        stride,
        facing,
        joint,
        tail_state,
    })
}

pub(super) fn alignment(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    owners: &[DesignParameterOwner],
    types: &StreamTypes<'_>,
) -> Result<Option<DesignAssemblyAlignment>, CodecError> {
    let Some(grammar) = parse(bytes, scope, types) else {
        return Ok(None);
    };
    let stream = native_stream(&scope.id);
    if grammar.stride == 11
        && types
            .get(&u64::from(scope.record_index))
            .is_some_and(|(_, version)| *version == 5)
        && owners
            .iter()
            .filter(|owner| {
                native_stream(owner.id()) == stream
                    && owner.scope_record_index() == scope.record_index
            })
            .count()
            != 4
    {
        return Ok(None);
    }
    let mut lanes = Vec::new();
    for index in [
        grammar.owners[0],
        grammar.owners[2],
        grammar.owners[3],
        grammar.owners[1],
    ] {
        let mut matches = owners.iter().filter(|owner| {
            native_stream(owner.id()) == stream
                && owner.scope_record_index() == scope.record_index
                && owner.record_index() == index
        });
        let Some(owner) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Ok(None);
        }
        lanes.push(owner);
    }
    let paths = exact_assembly_operand_paths(bytes, records, scope, types)?;
    let form = match paths {
        Some(paths)
            if grammar.joint.checked_add(1) == Some(scope.record_index)
                && (grammar.tail_state == [0, 0] || version_five(scope, types)) =>
        {
            DesignAssemblyAlignmentForm::qualified(
                grammar.frames,
                paths.map(|path| DesignAssemblyOperandQualifier::OccurrencePath { path }),
            )
        }
        _ => DesignAssemblyAlignmentForm::Frames {
            frames: grammar.frames,
        },
    };
    Ok(DesignAssemblyAlignment::try_new(
        lanes[0].evaluated_value().get(),
        [
            lanes[1].evaluated_value().get(),
            lanes[2].evaluated_value().get(),
            lanes[3].evaluated_value().get(),
        ],
        lanes
            .iter()
            .map(|owner| Located {
                value: owner.record_index(),
                offset: owner.evaluated_value_offset(),
            })
            .collect(),
        Some(form),
    )
    .map(|alignment| alignment.with_frame_sense(Some(grammar.facing)))
    .ok()
    .filter(|alignment| valid_alignment(scope, alignment, types, owners)))
}

pub(crate) fn alignment_layout(
    scope: &DesignParameterScope,
    alignment: &DesignAssemblyAlignment,
    types: &StreamTypes<'_>,
) -> Option<([usize; 2], usize)> {
    if !registered(scope, types) || alignment.frame_sense.is_none() {
        return None;
    }
    let frames = alignment.operand_frames()?;
    let start = scope.byte_offset();
    let layout = VersionLayout::for_scope(scope, types)?;
    let first = frames[0]
        .reference_offset
        .checked_sub(start)?
        .checked_sub(1)?;
    let expanded = if first == u64_from_index(prefix::LEN + 3 + layout.prologue_zeros) {
        0
    } else if first == u64_from_index(prefix::LEN + 7 + layout.prologue_zeros) {
        1
    } else {
        return None;
    };
    let first_table = *scope.reference_members().offsets().next()?;
    let last_table = *scope.reference_members().offsets().next_back()?;
    let table_span = u64::try_from(scope.reference_members().len().checked_sub(1)?).ok()?;
    let stride = [11_usize, crate::bytes::TYPED_LOCAL_REFERENCE_LEN]
        .into_iter()
        .find(|stride| {
            scope.kind_offset().checked_sub(last_table) == Some(u64_from_index(*stride + 7))
                && last_table.checked_sub(first_table)
                    == table_span.checked_mul(u64_from_index(*stride))
        })?;
    if layout.frame_guid && stride != 11 {
        return None;
    }
    let mut at = usize::try_from(first).ok()?;
    let mut matrices = 0;
    for frame in frames {
        if frame.reference_offset != start.checked_add(u64_from_index(at + 1))? {
            return None;
        }
        at = at.checked_add(stride)?.checked_add(layout.frame_zeros)?;
        if frame.transform_offset == start.checked_add(u64_from_index(at))? {
            at = at.checked_add(1)?;
            let identity: crate::records::sketch_placement::SketchPlacementMatrix =
                std::array::from_fn(|i| std::array::from_fn(|j| f64::from(u8::from(i == j))))
                    .try_into()
                    .ok()?;
            if frame.transform != identity {
                return None;
            }
        } else if frame.transform_offset == start.checked_add(u64_from_index(at + 1))? {
            at = at.checked_add(129)?;
            matrices += 1;
        } else {
            return None;
        }
        if layout.frame_guid {
            at = at.checked_add(76)?;
        }
        if !types.contains_key(&u64::from(frame.reference_record_index)) {
            return None;
        }
    }
    let first_locator = at
        .checked_add(14 + layout.angle_zeros)?
        .checked_add(4_usize.checked_mul(stride)?)?;
    let reference_count = first_locator
        .checked_add(3_usize.checked_mul(stride)?)?
        .checked_add(79)?;
    if scope.reference_count_offset() != start.checked_add(u64_from_index(reference_count))? {
        return None;
    }
    let span = 190_usize
        .checked_add(
            scope
                .reference_members()
                .len()
                .checked_add(12)?
                .checked_mul(stride)?,
        )?
        .checked_add(128 * matrices)?
        .checked_add(4 * expanded)?
        .checked_add(layout.extra_span(stride))?;
    if !(span..=span.checked_add(usize::from(!layout.tail_reference))?)
        .contains(&usize::try_from(scope.frame_length()).ok()?)
    {
        return None;
    }
    Some(([first_locator, first_locator.checked_add(stride)?], stride))
}

pub(crate) fn valid_alignment(
    scope: &DesignParameterScope,
    alignment: &DesignAssemblyAlignment,
    types: &StreamTypes<'_>,
    owners: &[DesignParameterOwner],
) -> bool {
    if types
        .get(&u64::from(scope.record_index))
        .is_some_and(|(_, version)| *version == 5)
        && owners
            .iter()
            .filter(|owner| {
                native_stream(owner.id()) == native_stream(&scope.id)
                    && owner.scope_record_index() == scope.record_index
            })
            .count()
            != 4
    {
        return false;
    }
    let Some((locators, _)) = alignment_layout(scope, alignment, types) else {
        return false;
    };
    let scalars = [
        alignment.angle(),
        alignment.offset()[0],
        alignment.offset()[1],
        alignment.offset()[2],
    ];
    if alignment.owners.len() != 4
        || !alignment.owners.iter().zip(scalars).all(|(lane, value)| {
            scope
                .reference_members()
                .values()
                .any(|index| *index == lane.value)
                && owners
                    .iter()
                    .filter(|owner| {
                        native_stream(owner.id()) == native_stream(&scope.id)
                            && owner.scope_record_index() == scope.record_index
                            && owner.record_index() == lane.value
                            && owner.evaluated_value_offset() == lane.offset
                            && owner.evaluated_value().get() == value
                    })
                    .count()
                    == 1
        })
    {
        return false;
    }
    if let Some(paths) = alignment.operand_path_refs() {
        for (path, relative) in paths.into_iter().zip(locators) {
            let link = path.link();
            let Some(layout) = path
                .byte_offset()
                .checked_sub(link.locator_byte_offset)
                .and_then(|length| usize::try_from(length).ok())
                .and_then(|length| {
                    crate::design::assembly::RigidGroupLocatorLayout::admit_assemble(
                        types.get(&u64::from(link.locator_record_index)).copied(),
                        length,
                    )
                })
            else {
                return false;
            };
            let (_, backlink, wrapper, _, _) = layout.offsets();
            if !path.admitted_by_type()
                || Some(link.locator_reference_offset)
                    != scope
                        .byte_offset()
                        .checked_add(u64_from_index(relative + 1))
                || Some(link.locator_scope_reference_offset)
                    != link
                        .locator_byte_offset
                        .checked_add(u64_from_index(backlink + 1))
                || Some(link.wrapper_reference_offset)
                    != link
                        .locator_byte_offset
                        .checked_add(u64_from_index(wrapper + 1))
                || link.locator_record_index.checked_add(1) != Some(path.record_index)
                || link
                    .wrapper_record_index
                    .checked_sub(link.locator_record_index)
                    .is_none_or(|span| !(2..=65).contains(&span))
                || Some(link.path_reference_offset) != link.wrapper_byte_offset.checked_add(27)
            {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests;
