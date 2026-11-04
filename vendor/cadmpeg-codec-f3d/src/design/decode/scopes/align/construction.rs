// SPDX-License-Identifier: Apache-2.0
//! Read the complete fixed Align constructor without assigning operand roles.

use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::layout::align_constructor as layout;
use crate::records::feature::align::{DesignAlignConstruction, DesignAlignEnvelope};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, View};
use cadmpeg_ir::scalar::FiniteReal;

pub(crate) const WORD_OFFSETS: [usize; 8] = [
    layout::FIRST_WORD,
    layout::SECOND_WORD,
    layout::THIRD_WORD,
    layout::FOURTH_WORD,
    layout::OWNER_WORD,
    layout::FIRST_COMPANION_WORD,
    layout::SECOND_COMPANION_WORD,
    layout::REPEATED_OWNER_WORD,
];
pub(crate) const CODE_OFFSETS: [usize; 5] = [
    layout::FIRST_CODE,
    layout::SECOND_CODE,
    layout::THIRD_CODE,
    layout::FIRST_SCALAR_CODE,
    layout::SECOND_SCALAR_CODE,
];

pub(crate) fn align_construction_links_hold(
    scope: &DesignParameterScope,
    envelope: &DesignAlignEnvelope,
    construction: &DesignAlignConstruction,
) -> bool {
    let offset = |at| construction.byte_offset.checked_add(u64_from_index(at));
    construction.record_index == envelope.first_reference.value
        && offset(layout::LEN) == Some(construction.paired_byte_offset)
        && construction
            .marked_words
            .iter()
            .zip(WORD_OFFSETS)
            .all(|(word, at)| Some(word.offset) == offset(at))
        && construction
            .codes
            .iter()
            .zip(CODE_OFFSETS)
            .all(|(code, at)| Some(code.offset) == offset(at))
        && Some(construction.matrix_scalars.offset) == offset(layout::MATRIX_SCALARS)
        && Some(construction.scalar.offset) == offset(layout::SCALAR)
        && construction.marked_words[4].value == scope.record_index
        && construction.marked_words[7].value == scope.record_index
}

pub(in crate::design) fn decode_align_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    envelope: &DesignAlignEnvelope,
) -> Option<DesignAlignConstruction> {
    let record_index = envelope.first_reference.value;
    let start = records.first_at_or_after(0, record_index)?;
    let paired = records.first_at_or_after(
        start.checked_add(crate::layout::indexed_design_record_header::LEN)?,
        record_index,
    )?;
    if paired.checked_sub(start)? != layout::LEN {
        return None;
    }
    let frame = View::over_retained(bytes).child(start, paired)?;
    let read_class = |at| {
        let (class, end) = lp_ascii_filtered_view(bytes, at, 3..=3, u8::is_ascii_digit)?;
        (View::u32_le_at(bytes, end)? == record_index).then_some(())?;
        crate::design::decode::text::class_tag_from_view(class).ok()
    };
    let class_tag = read_class(start)?;
    let paired_class_tag = read_class(paired)?;
    for at in [
        layout::FIRST_MARKER,
        layout::SECOND_MARKER,
        layout::THIRD_MARKER,
        layout::FOURTH_MARKER,
        layout::OWNER_MARKER,
        layout::FIRST_COMPANION_MARKER,
        layout::SECOND_COMPANION_MARKER,
        layout::REPEATED_OWNER_MARKER,
    ] {
        if frame.window().get(at) != Some(&layout::FIRST_MARKER_VALUE) {
            return None;
        }
    }
    for (at, expected) in [
        (layout::HEADER_ZERO, layout::HEADER_ZERO_VALUE.as_slice()),
        (
            layout::FIRST_PADDING,
            layout::FIRST_PADDING_VALUE.as_slice(),
        ),
        (
            layout::SECOND_PADDING,
            layout::SECOND_PADDING_VALUE.as_slice(),
        ),
        (
            layout::THIRD_PADDING,
            layout::THIRD_PADDING_VALUE.as_slice(),
        ),
        (
            layout::FOURTH_PADDING,
            layout::FOURTH_PADDING_VALUE.as_slice(),
        ),
        (layout::MATRIX_ZERO, layout::MATRIX_ZERO_VALUE.as_slice()),
        (
            layout::MATRIX_TAIL_ZERO,
            layout::MATRIX_TAIL_ZERO_VALUE.as_slice(),
        ),
        (layout::OWNER_ZERO, layout::OWNER_ZERO_VALUE.as_slice()),
        (
            layout::OWNER_PADDING,
            layout::OWNER_PADDING_VALUE.as_slice(),
        ),
        (
            layout::FIRST_COMPANION_PADDING,
            layout::FIRST_COMPANION_PADDING_VALUE.as_slice(),
        ),
        (
            layout::SECOND_COMPANION_PADDING,
            layout::SECOND_COMPANION_PADDING_VALUE.as_slice(),
        ),
        (
            layout::REPEATED_OWNER_PADDING,
            layout::REPEATED_OWNER_PADDING_VALUE.as_slice(),
        ),
    ] {
        if frame.window().get(at..at.checked_add(expected.len())?) != Some(expected) {
            return None;
        }
    }
    let read_word = |at| {
        let at = start.checked_add(at)?;
        Some(Located {
            value: View::u32_le_at(bytes, at)?,
            offset: u64_from_index(at),
        })
    };
    let read_scalar = |at| {
        let at = start.checked_add(at)?;
        Some(Located {
            value: FiniteReal::new(View::f64_le_at(bytes, at)?)?,
            offset: u64_from_index(at),
        })
    };
    let matrix_at = start.checked_add(layout::MATRIX_SCALARS)?;
    let mut matrix = [FiniteReal::new(0.0)?; 16];
    for (ordinal, value) in matrix.iter_mut().enumerate() {
        let at = matrix_at.checked_add(ordinal.checked_mul(std::mem::size_of::<f64>())?)?;
        *value = FiniteReal::new(View::f64_le_at(bytes, at)?)?;
    }
    let construction = DesignAlignConstruction {
        record_index,
        byte_offset: u64_from_index(start),
        paired_byte_offset: u64_from_index(paired),
        class_tag,
        paired_class_tag,
        marked_words: [
            read_word(WORD_OFFSETS[0])?,
            read_word(WORD_OFFSETS[1])?,
            read_word(WORD_OFFSETS[2])?,
            read_word(WORD_OFFSETS[3])?,
            read_word(WORD_OFFSETS[4])?,
            read_word(WORD_OFFSETS[5])?,
            read_word(WORD_OFFSETS[6])?,
            read_word(WORD_OFFSETS[7])?,
        ],
        codes: [
            read_word(CODE_OFFSETS[0])?,
            read_word(CODE_OFFSETS[1])?,
            read_word(CODE_OFFSETS[2])?,
            read_word(CODE_OFFSETS[3])?,
            read_word(CODE_OFFSETS[4])?,
        ],
        matrix_scalars: Located {
            value: matrix,
            offset: u64_from_index(matrix_at),
        },
        scalar: read_scalar(layout::SCALAR)?,
    };
    align_construction_links_hold(scope, envelope, &construction).then_some(construction)
}

#[cfg(test)]
mod tests;
