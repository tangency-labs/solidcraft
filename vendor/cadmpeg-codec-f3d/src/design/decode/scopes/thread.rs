// SPDX-License-Identifier: Apache-2.0
//! Exact thread construction scopes and thread payloads.

use crate::bytes::lp_utf16_bounded;
use crate::layout::thread_compact_construction_tail as thread_compact_tail;
use crate::layout::thread_compact_legacy_construction_tail as thread_compact_legacy_tail;
use crate::layout::thread_extended_compact_trailer as extended_compact;
use crate::layout::thread_extended_scope_prefix as thread_extended;
use crate::layout::thread_extended_standard_trailer as extended_standard;
use crate::layout::thread_owner_marked_scope_prefix as thread_owner;
use crate::layout::thread_standard_construction_tail as thread_tail;
use crate::layout::thread_standard_legacy_construction_tail as thread_standard_legacy_tail;
use crate::layout::thread_standard_scope_prefix as thread_standard;
use crate::records::feature::scope;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::feature::thread;
use crate::records::feature::thread::DesignThreadConstruction;
use crate::records::feature::thread::DesignThreadForm;
use cadmpeg_core::decode::View;

pub(super) fn exact_thread_construction(
    bytes: &[u8],
    scope: &DesignParameterScope,
    owners: &[crate::records::parameters::DesignParameterOwner],
) -> Option<DesignThreadConstruction> {
    let start = usize::try_from(scope.byte_offset()).ok()?;
    if scope.kind() != scope::DesignFeatureKind::Thread
        || scope.reference_members().len() < 2
        || !scope.reference_members().len().is_multiple_of(2)
    {
        return None;
    }
    let (prefix_form, designation_delta) = bytes.get(start..).and_then(exact_thread_prefix)?;
    let designation_at = start.checked_add(designation_delta)?;
    let reference_count = scope.reference_members().len();

    let mut face_group_record_indices = Vec::new();
    match prefix_form {
        ThreadPrefix::Standard => {
            let first = scope.reference_members().values().next()?;
            face_group_record_indices.push(*first);
        }
        ThreadPrefix::Compact => {
            face_group_record_indices
                .extend(scope.reference_members().values().step_by(2).copied());
        }
        ThreadPrefix::Extended(_) => {}
    }
    let mut construction = parse_thread_payload(
        bytes,
        designation_at,
        &prefix_form,
        face_group_record_indices,
    )?;
    if matches!(
        construction.form,
        DesignThreadForm::StandardExtended | DesignThreadForm::CompactExtended(_)
    ) {
        let references = scope
            .reference_members()
            .values()
            .copied()
            .collect::<Vec<_>>();
        construction.face_group_record_indices = construction.form.face_groups(&references);
    }
    if matches!(
        construction.form,
        DesignThreadForm::StandardExtended | DesignThreadForm::CompactExtended(_)
    ) {
        let (_, after_designation) = lp_utf16_bounded(bytes, designation_at, 1..=128)?;
        let (_, after_nominal) = lp_utf16_bounded(bytes, after_designation, 1..=64)?;
        let (_, after_profile) = lp_utf16_bounded(bytes, after_nominal, 1..=256)?;
        let trailer = after_profile.checked_add(thread_tail::STANDARD_TRAILER)?;
        let (length_at, offset_at, length_ordinal, offset_ordinal) = match construction.form {
            DesignThreadForm::StandardExtended => (
                extended_standard::LENGTH_REFERENCE,
                extended_standard::OFFSET_REFERENCE,
                reference_count.checked_sub(1)?,
                reference_count.checked_sub(2)?,
            ),
            DesignThreadForm::CompactExtended(_) => (
                extended_compact::LENGTH_REFERENCE,
                extended_compact::OFFSET_REFERENCE,
                1,
                0,
            ),
            _ => return None,
        };
        if super::shared_frames::marked_record_reference(bytes, trailer + length_at)
            != scope
                .reference_members()
                .values()
                .nth(length_ordinal)
                .copied()
            || super::shared_frames::marked_record_reference(bytes, trailer + offset_at)
                != scope
                    .reference_members()
                    .values()
                    .nth(offset_ordinal)
                    .copied()
        {
            return None;
        }
    }
    if !construction.form.owners_hold(scope, owners)
        || !construction
            .form
            .class_pair_holds(scope.class_tag.as_str(), scope.paired_class_tag.as_str())
        || !construction
            .form
            .designation_offset_holds(construction.designation_offset, scope.byte_offset())
        || !construction
            .form
            .reference_count_holds(scope.reference_members().len())
    {
        return None;
    }
    Some(construction)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ThreadPrefix {
    Standard,
    Compact,
    /// The extended prefix and the fields it states before the designation.
    Extended(ExtendedThreadPrefix),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExtendedThreadPrefix {
    external: bool,
    modeled: bool,
    class: String,
    designation_copy: String,
}

/// Byte length of each entry that the extended prefix counts at offset 30.
const EXTENDED_PREFIX_ENTRY_LEN: usize = 16;

/// Longest thread class, in UTF-16 code units, that the extended prefix admits.
const THREAD_CLASS_UNITS: usize = 16;

fn exact_thread_prefix(bytes: &[u8]) -> Option<(ThreadPrefix, usize)> {
    let direct = if bytes.get(thread_standard::ZERO_RUN_10..thread_standard::FIXED_SCALAR)?
        == [0; 10]
        && View::f64_le_at(bytes, thread_standard::FIXED_SCALAR)?.to_bits() == 60.0f64.to_bits()
    {
        thread_form(
            bytes,
            thread_standard::STANDARD_MARKER,
            thread_standard::STANDARD_PREFIX_TAIL,
        )
        .map(|form| (form, thread_standard::LEN))
    } else {
        None
    };
    let owner_marked = if bytes.get(thread_owner::ZERO_RUN_9..thread_owner::OWNER_MARKER)? == [0; 9]
        && View::u32_le_at(bytes, thread_owner::OWNER_MARKER)? == 1
        && bytes.get(thread_owner::SEPARATOR) == Some(&0)
        && View::f64_le_at(bytes, thread_owner::FIXED_SCALAR)?.to_bits() == 60.0f64.to_bits()
    {
        thread_form(bytes, thread_owner::FORM_MARKER, thread_owner::FORM_TOKEN)
            .map(|form| (form, thread_owner::LEN))
    } else {
        None
    };
    let extended = extended_thread_prefix(bytes);
    let mut candidates = [direct, owner_marked, extended].into_iter().flatten();
    let prefix = candidates.next()?;
    candidates.next().is_none().then_some(prefix)
}

/// The extended prefix: the external flag, the counted entries, the LP-UTF16
/// thread class, and the LP-UTF16 designation copy before the designation.
fn extended_thread_prefix(bytes: &[u8]) -> Option<(ThreadPrefix, usize)> {
    if bytes.get(thread_extended::ZERO_RUN_10..thread_extended::FIXED_SCALAR)? != [0; 10]
        || View::f64_le_at(bytes, thread_extended::FIXED_SCALAR)?.to_bits() != 60.0f64.to_bits()
    {
        return None;
    }
    let external = match bytes.get(thread_extended::EXTERNAL_FLAG)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let modeled = match View::u32_le_at(bytes, thread_extended::ENTRY_COUNT)? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let class_at = thread_extended::LEN + usize::from(modeled) * EXTENDED_PREFIX_ENTRY_LEN;
    let (class, copy_at) = lp_utf16_bounded(bytes, class_at, 1..=THREAD_CLASS_UNITS)?;
    let (designation_copy, designation_at) = lp_utf16_bounded(bytes, copy_at, 0..=128)?;
    Some((
        ThreadPrefix::Extended(ExtendedThreadPrefix {
            external,
            modeled,
            class,
            designation_copy,
        }),
        designation_at,
    ))
}

fn thread_form(bytes: &[u8], marker_at: usize, token_at: usize) -> Option<ThreadPrefix> {
    match (
        bytes.get(marker_at..marker_at + 5)?,
        bytes.get(token_at..token_at + 4)?,
    ) {
        ([1, 2, 0, 0, 0], [0x36, 0, 0x67, 0]) => Some(ThreadPrefix::Standard),
        ([0, 2, 0, 0, 0], [0x36, 0, 0x48, 0]) => Some(ThreadPrefix::Compact),
        _ => None,
    }
}

pub(super) fn parse_thread_payload(
    bytes: &[u8],
    designation_at: usize,
    expected_form: &ThreadPrefix,
    face_group_record_indices: Vec<u32>,
) -> Option<DesignThreadConstruction> {
    let (designation, after_designation) = lp_utf16_bounded(bytes, designation_at, 1..=128)?;
    let (nominal_size_text, after_nominal) = lp_utf16_bounded(bytes, after_designation, 1..=64)?;
    let (profile, after_profile) = lp_utf16_bounded(bytes, after_nominal, 1..=256)?;
    (|| {
        let extended = match expected_form {
            ThreadPrefix::Extended(prefix) => Some(prefix),
            ThreadPrefix::Standard | ThreadPrefix::Compact => None,
        };
        if extended.is_some_and(|prefix| {
            !prefix.designation_copy.is_empty() && prefix.designation_copy != designation
        }) {
            return None;
        }
        let (pitch_marker, trailer_kind, full_length) =
            match (expected_form, bytes.get(after_profile..after_profile + 5)?) {
                (ThreadPrefix::Extended(prefix), [flag @ (0 | 1), 1, 0, 0, 0]) => (
                    u8::from(prefix.modeled),
                    ThreadTrailerKind::ExtendedStandard,
                    Some(*flag == 1),
                ),
                (ThreadPrefix::Standard, [0, 1, 0, 0, 0]) => (1, ThreadTrailerKind::Standard, None),
                (ThreadPrefix::Standard, [1, 1, 0, 0, 0]) => {
                    (0, ThreadTrailerKind::StandardLegacy, None)
                }
                (ThreadPrefix::Standard, [1, 2, 0, 0, 0]) => {
                    (0, ThreadTrailerKind::StandardCompact, None)
                }
                (ThreadPrefix::Compact, [1, 2, 0, 0, 0]) => (0, ThreadTrailerKind::Compact, None),
                (ThreadPrefix::Extended(prefix), [1, 2, 0, 0, 0]) if !prefix.modeled => {
                    (0, ThreadTrailerKind::Compact, None)
                }
                (ThreadPrefix::Compact, [1, 1, 0, 0, 0]) => {
                    (0, ThreadTrailerKind::CompactLegacy, None)
                }
                _ => return None,
            };
        let nominal_size = thread::DesignThreadNominalSize::try_from(nominal_size_text).ok()?;
        let major_diameter = View::f64_le_at(bytes, after_profile + thread_tail::MAJOR_DIAMETER)?;
        let minor_diameter = View::f64_le_at(bytes, after_profile + thread_tail::MINOR_DIAMETER)?;
        let pitch = (bytes.get(after_profile + thread_tail::PITCH_MARKER) == Some(&pitch_marker))
            .then(|| View::f64_le_at(bytes, after_profile + thread_tail::PITCH))??;
        let pitch_diameter = View::f64_le_at(bytes, after_profile + thread_tail::PITCH_DIAMETER)?;
        let trailer_at = match trailer_kind {
            ThreadTrailerKind::Standard => thread_tail::STANDARD_TRAILER,
            ThreadTrailerKind::Compact => thread_compact_tail::COMPACT_TRAILER,
            ThreadTrailerKind::StandardLegacy => thread_standard_legacy_tail::LEGACY_TRAILER,
            ThreadTrailerKind::CompactLegacy => thread_compact_legacy_tail::LEGACY_TRAILER,
            ThreadTrailerKind::StandardCompact | ThreadTrailerKind::ExtendedStandard => {
                thread_tail::STANDARD_TRAILER
            }
        };
        let trailer_offset = after_profile.checked_add(trailer_at)?;
        let form = match trailer_kind {
            ThreadTrailerKind::ExtendedStandard
                if bytes.get(trailer_offset + extended_standard::ZERO_FLAG)
                    == Some(&extended_standard::ZERO_FLAG_VALUE)
                    && bytes.get(trailer_offset + extended_standard::END_FLAG)
                        == Some(&extended_standard::END_FLAG_VALUE) =>
            {
                DesignThreadForm::StandardExtended
            }
            ThreadTrailerKind::Standard
                if bytes.get(trailer_offset..trailer_offset + 2)? == [0, 1] =>
            {
                DesignThreadForm::Standard
            }
            ThreadTrailerKind::StandardLegacy
                if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
            {
                DesignThreadForm::StandardLegacy
            }
            ThreadTrailerKind::CompactLegacy
                if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
            {
                DesignThreadForm::CompactLegacy
            }
            ThreadTrailerKind::StandardCompact
                if standard_compact_trailer_is_valid(
                    bytes,
                    trailer_offset,
                    face_group_record_indices.first().copied()?,
                ) =>
            {
                DesignThreadForm::StandardCompact
            }
            ThreadTrailerKind::Compact
                if bytes.get(trailer_offset..trailer_offset + 4)? == [0, 0, 0, 1] =>
            {
                DesignThreadForm::Compact(None)
            }
            ThreadTrailerKind::Compact
                if bytes.get(trailer_offset) == Some(&1)
                    && bytes.get(trailer_offset + 5..trailer_offset + 11)? == [0; 6] =>
            {
                let reference_offset = trailer_offset.checked_add(1)?;
                let record_index =
                    std::num::NonZeroU32::new(View::u32_le_at(bytes, reference_offset)?)?;
                let reference = crate::records::identity::Located {
                    value: record_index,
                    offset: u64::try_from(reference_offset).ok()?,
                };
                if extended.is_some() {
                    (bytes.get(trailer_offset + extended_compact::END_FLAG)
                        == Some(&extended_compact::END_FLAG_VALUE))
                    .then_some(())?;
                    DesignThreadForm::CompactExtended(reference)
                } else {
                    DesignThreadForm::Compact(Some(reference))
                }
            }
            _ => return None,
        };
        if extended.is_some()
            && !matches!(
                form,
                DesignThreadForm::CompactExtended(_) | DesignThreadForm::StandardExtended
            )
        {
            return None;
        }
        Some(DesignThreadConstruction {
            form,
            designation_offset: u64::try_from(designation_at).ok()?,
            designation: cadmpeg_core::text::NonBlankString::new(designation)?,
            nominal_size,
            profile: cadmpeg_core::text::NonBlankString::new(profile)?,
            pitch: cadmpeg_ir::scalar::PositiveReal::new(pitch)?,
            face_group_record_indices,
            diameters: thread::DesignThreadDiameters::new(
                major_diameter,
                minor_diameter,
                pitch_diameter,
            )?,
            extended: extended.map(|prefix| thread::DesignThreadExtended {
                class: prefix.class.clone(),
                external: prefix.external,
                modeled: prefix.modeled,
                full_length,
            }),
        })
    })()
}

/// Admit the trailer of the standard-prefix compact-marker form: a run of two
/// or three zero bytes, the byte `01`, u32 `1`, and a marked reference to the
/// face-selection group followed by four zero bytes.
fn standard_compact_trailer_is_valid(
    bytes: &[u8],
    trailer_offset: usize,
    face_group_record_index: u32,
) -> bool {
    let Some(tail) = bytes.get(trailer_offset..) else {
        return false;
    };
    let zero_run = tail.iter().take_while(|byte| **byte == 0).count();
    if !STANDARD_COMPACT_ZERO_RUNS.contains(&zero_run) {
        return false;
    }
    let Some(flag_at) = trailer_offset.checked_add(zero_run) else {
        return false;
    };
    let Some(count_at) = flag_at.checked_add(1) else {
        return false;
    };
    let Some(marker_at) = count_at.checked_add(4) else {
        return false;
    };
    let Some(index_at) = marker_at.checked_add(1) else {
        return false;
    };
    bytes.get(flag_at) == Some(&1)
        && View::u32_le_at(bytes, count_at) == Some(1)
        && bytes.get(marker_at) == Some(&1)
        && View::u32_le_at(bytes, index_at) == Some(face_group_record_index)
        && index_at
            .checked_add(4)
            .and_then(|padding_at| {
                bytes.get(padding_at..padding_at.checked_add(REFERENCE_PADDING)?)
            })
            .is_some_and(|padding| padding.iter().all(|byte| *byte == 0))
}

/// Zero bytes after the u32 record index of the trailer's marked reference.
const REFERENCE_PADDING: usize = 4;

/// Zero-run lengths before the trailer `01` of the standard-prefix compact-marker form.
const STANDARD_COMPACT_ZERO_RUNS: std::ops::RangeInclusive<usize> = 2..=3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThreadTrailerKind {
    Standard,
    Compact,
    StandardLegacy,
    CompactLegacy,
    StandardCompact,
    ExtendedStandard,
}
