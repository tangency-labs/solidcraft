// SPDX-License-Identifier: Apache-2.0
//! Decode the fixed Align prefix independently of operand interpretation.

pub(crate) mod construction;

use crate::bytes::lp_utf16_bounded;
use crate::layout::{align_scope_extended_prefix as extended, align_scope_prefix as compact};
use crate::records::feature::align::{DesignAlignEnvelope, DesignAlignForm};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use cadmpeg_core::decode::{u64_from_index, View};

pub(crate) fn align_envelope_links_hold(
    scope: &DesignParameterScope,
    envelope: &DesignAlignEnvelope,
) -> bool {
    let shift = match envelope.form {
        DesignAlignForm::Compact => 0,
        DesignAlignForm::Extended => extended::LEN - compact::LEN,
    };
    let offset = |at| scope.byte_offset().checked_add(u64_from_index(at + shift));
    scope.kind() == DesignFeatureKind::Align
        && Some(scope.reference_count_offset()) == offset(compact::LEN)
        && Some(envelope.first_reference.offset) == offset(compact::FIRST_REFERENCE)
        && Some(envelope.second_reference.offset) == offset(compact::SECOND_REFERENCE)
        && Some(envelope.source_code.offset) == offset(compact::SOURCE_CODE)
        && scope.reference_members().values().nth(2) == Some(&envelope.first_reference.value)
        && scope.reference_members().values().next() == Some(&envelope.second_reference.value)
        && envelope.construction.as_ref().is_none_or(|construction| {
            self::construction::align_construction_links_hold(scope, envelope, construction)
        })
}

pub(crate) fn decode_align_envelope(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> Option<DesignAlignEnvelope> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    let length = usize::try_from(
        scope
            .reference_count_offset()
            .checked_sub(scope.byte_offset())?,
    )
    .ok()?;
    let (form, shift) = if length == compact::LEN {
        (DesignAlignForm::Compact, 0)
    } else if length == extended::LEN {
        if View::u32_le_at(bytes, start.checked_add(extended::EXTRA_CODE)?)?
            != extended::EXTRA_CODE_VALUE
        {
            return None;
        }
        (DesignAlignForm::Extended, extended::LEN - compact::LEN)
    } else {
        return None;
    };
    let view = View::over_retained(bytes).child(start, start.checked_add(length)?)?;
    if view.window().get(compact::HEADER_ZERO..compact::ZERO) != Some(&compact::HEADER_ZERO_VALUE) {
        return None;
    }
    let at = |offset| start.checked_add(offset)?.checked_add(shift);
    for (offset, expected) in [
        (compact::ZERO, compact::ZERO_VALUE),
        (compact::FIRST_MARKER, compact::FIRST_MARKER_VALUE),
        (compact::SECOND_MARKER, compact::SECOND_MARKER_VALUE),
    ] {
        if bytes.get(at(offset)?) != Some(&expected) {
            return None;
        }
    }
    for (offset, expected) in [
        (
            compact::FIRST_PADDING,
            compact::FIRST_PADDING_VALUE.as_slice(),
        ),
        (
            compact::SECOND_PADDING,
            compact::SECOND_PADDING_VALUE.as_slice(),
        ),
        (compact::TAIL_ZERO, compact::TAIL_ZERO_VALUE.as_slice()),
    ] {
        let offset = at(offset)?;
        if bytes.get(offset..offset.checked_add(expected.len())?) != Some(expected) {
            return None;
        }
    }
    let identity_length = usize::try_from(compact::IDENTITY_LENGTH_VALUE).ok()?;
    let (identity, end) = lp_utf16_bounded(
        bytes,
        at(compact::IDENTITY_LENGTH)?,
        identity_length..=identity_length,
    )?;
    if identity != "00000000-0000-0000-0000-000000000000" || end != at(compact::TAIL_ZERO)? {
        return None;
    }
    let read = |offset| {
        let offset = at(offset)?;
        Some(Located {
            value: View::u32_le_at(bytes, offset)?,
            offset: u64_from_index(offset),
        })
    };
    let envelope = DesignAlignEnvelope {
        form,
        first_reference: read(compact::FIRST_REFERENCE)?,
        source_code: read(compact::SOURCE_CODE)?,
        second_reference: read(compact::SECOND_REFERENCE)?,
        construction: None,
    };
    align_envelope_links_hold(scope, &envelope).then_some(envelope)
}

#[cfg(test)]
mod tests;

pub(super) fn align_native_loss(
    scope: &DesignParameterScope,
    envelope: Option<&DesignAlignEnvelope>,
) -> cadmpeg_ir::report::loss::LossNote {
    let (code, reason) = match envelope {
        Some(envelope) if envelope.construction.is_some() => (
            crate::loss::F3dLossCode::AlignOperandsUnresolved,
            "typed constructor retained; operand roles and motion relation have no neutral binding",
        ),
        Some(_) => (
            crate::loss::F3dLossCode::AlignConstructionUnparsed,
            "typed prefix retained; referenced constructor has no admitted layout",
        ),
        None => (
            crate::loss::F3dLossCode::AlignEnvelopeUnparsed,
            "source envelope retained; specialized prefix has no admitted layout",
        ),
    };
    code.note(format!("Align scope {}: {reason}.", scope.id))
}
