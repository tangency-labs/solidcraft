// SPDX-License-Identifier: Apache-2.0
//! Parse Emboss construction links and retain the control lanes without assigning enums.

use crate::layout::{
    emboss_counted_suffix as suffix, emboss_native_pair as pair, emboss_two_control_head as head,
};
use crate::records::feature::emboss::{DesignEmbossConstruction, DesignEmbossControlWords};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;

struct ParsedEmboss<'a> {
    construction: DesignEmbossConstruction,
    additional_pairs: View<'a>,
    pair_count: usize,
}

pub(super) fn exact_emboss_construction(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Result<Option<DesignEmbossConstruction>, CodecError> {
    if scope.kind() != DesignFeatureKind::Emboss {
        return Ok(None);
    }
    let ((Some(parsed), None) | (None, Some(parsed))) = (
        parse_form(bytes, scope, false),
        parse_form(bytes, scope, true),
    ) else {
        return Ok(None);
    };
    let mut construction = parsed.construction;
    if parsed.pair_count > 1 {
        let mut values = ctx.alloc_filled(
            parsed.pair_count - 1,
            [0; 2],
            "decode F3D Emboss native pairs",
        )?;
        let mut pairs = parsed.additional_pairs;
        for value in &mut values {
            let Some(pair) = read_native_pair(&mut pairs) else {
                return Ok(None);
            };
            *value = pair;
        }
        construction.additional_native_pairs = Some(values);
    }
    Ok(Some(construction))
}

fn parse_form<'a>(
    bytes: &'a [u8],
    scope: &DesignParameterScope,
    one_word: bool,
) -> Option<ParsedEmboss<'a>> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let end = usize::try_from(scope.paired_byte_offset()).ok()?;
    let mut view = View::over_retained(bytes).child(start, end)?;
    let class_tag = scope.class_tag.as_str().as_bytes();
    if view.u32_le()? != u32::try_from(class_tag.len()).ok()?
        || view.take(class_tag.len())? != class_tag
        || view.u32_le()? != scope.record_index
        || view.take(head::ZERO_RUN_VALUE.len())? != head::ZERO_RUN_VALUE
    {
        return None;
    }
    let first = view.u32_le()?;
    let control_words = if one_word {
        DesignEmbossControlWords::One([first])
    } else {
        DesignEmbossControlWords::Two([first, view.u32_le()?])
    };
    let depth_owner = reference(&mut view)?;
    let auxiliary_group = reference(&mut view)?;
    let horizontal_owner = reference(&mut view)?;
    let pair_count = view.u32_le()?;
    let pair_count = view.counted(u64::from(pair_count), pair::LEN)?.get();
    let pair_start = view.position();
    let pair_end = pair_start.checked_add(pair_count.checked_mul(pair::LEN)?)?;
    let mut pairs = view.child(pair_start, pair_end)?;
    view.seek(pair_end)?;
    let native_pair = if pair_count == 0 {
        None
    } else {
        Some(read_native_pair(&mut pairs)?)
    };
    let rotation_owner = reference(&mut view)?;
    let face_group = reference(&mut view)?;
    let vertical_owner = reference(&mut view)?;
    if view.u32_le()? != suffix::PROFILE_GROUP_COUNT_VALUE {
        return None;
    }
    let profile_group = reference(&mut view)?;
    if view.take(suffix::ZERO_TAIL_VALUE.len())? != suffix::ZERO_TAIL_VALUE
        || view.u32_le()? != suffix::FACE_GROUP_COUNT_VALUE
    {
        return None;
    }
    if reference(&mut view)?.value != face_group.value
        || view.u32_le()? != suffix::NULL_GUID_COUNT_VALUE
    {
        return None;
    }
    for unit in "00000000-0000-0000-0000-000000000000".encode_utf16() {
        if view.u16_le()? != unit {
            return None;
        }
    }
    if view.take(suffix::ZERO_SEPARATOR_VALUE.len())? != suffix::ZERO_SEPARATOR_VALUE
        || scope.reference_count_offset() != u64_from_index(view.position())
    {
        return None;
    }
    let construction = DesignEmbossConstruction {
        control_words,
        native_pair,
        additional_native_pairs: None,
        depth_owner,
        auxiliary_group,
        horizontal_owner,
        rotation_owner,
        face_group,
        vertical_owner,
        profile_group,
        sketch_profile: None,
    };
    construction
        .links_match_for_pair_count(scope, pair_count)
        .then_some(ParsedEmboss {
            construction,
            additional_pairs: pairs,
            pair_count,
        })
}

fn read_native_pair(view: &mut View<'_>) -> Option<[u64; 2]> {
    Some([view.u64_le()?, view.u64_le()?])
}

fn reference(view: &mut View<'_>) -> Option<Located<u32>> {
    if view.u8()? != 1 {
        return None;
    }
    let offset = u64_from_index(view.position());
    let value = u32::try_from(view.u64_le()?).ok()?;
    if value == 0 || view.take(2)? != [0; 2] {
        return None;
    }
    Some(Located { value, offset })
}

#[cfg(test)]
mod tests;
