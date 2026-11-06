// SPDX-License-Identifier: Apache-2.0
//! Parse Design parameter, owner, and companion frames.

use crate::bytes::lp_utf16_bounded;
use cadmpeg_core::container::ContainerRole;

use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::design::decode::body::decode_stream;
use crate::design::decode::dimension_frames::companion_owned_interval;
use crate::design::decode::sketch::{next_indexed_record_offset, IndexedRecordOffsets};
use crate::design::decode::text::design_record_id;
use crate::ids::{self, native_stream};
use crate::layout::design_parameter_compact_owned_expression_trailer as compact_trailer;
use crate::layout::design_parameter_legacy_287_prefix as legacy_287;
use crate::layout::design_parameter_owner_legacy_68 as legacy_owner_68;
use crate::layout::design_parameter_owner_legacy_88 as legacy_owner_88;
use crate::layout::design_parameter_owner_prefix as owner_prefix;
use crate::layout::design_parameter_tail as parameter_tail;
use crate::layout::design_parameter_typed_compact_owned_prefix as typed;
use crate::layout::design_parameter_wide_expression_trailer as wide_trailer;
use crate::layout::indexed_companion_record_prefix as companion_prefix;
use crate::layout::indexed_design_record_header as indexed_header;
use crate::layout::typed_reference_companion_prefix as typed_companion;
use crate::records::{
    decal::DesignRecordHeader,
    entity_header::DesignEntityHeader,
    feature::scope::DesignParameterScope,
    parameters::{DesignParameter, DesignParameterCompanion, DesignParameterOwner},
    recipes::ConstructionRecipe,
};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

macro_rules! parameter_text {
    ($payload:expr, $at:expr, $bounds:expr) => {
        lp_utf16_bounded($payload, $at, $bounds)?
    };
}

/// Decode every parametric construction-recipe record (`body_recipe_data`,
/// `face_recipe_data`, `bounded_face_recipe_data`, `edge_recipe_data`,
/// `vertex_recipe_data`) from each design `BulkStream` entry in `scan`.
/// `recipe_index` is assigned per `(kind, design_id)` group in stream order.
pub(crate) fn decode_recipes(scan: &ContainerScan) -> Result<Vec<ConstructionRecipe>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        decode_stream(bytes, &entry.name, &mut out)?;
    }
    Ok(out)
}

/// Parameter frames in one Design `BulkStream` that match no parameter grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnparsedParameterRecords {
    /// Design `BulkStream` entry name.
    pub(crate) stream: String,
    /// Number of distinct unparsed frames.
    pub(crate) count: usize,
}

/// Decoded Design parameters and the parameter frames left unparsed.
#[derive(Debug, Default)]
pub(crate) struct DecodedParameters {
    /// Canonical parameter records, sorted by identity.
    pub(crate) parameters: Vec<DesignParameter>,
    /// Per-stream counts of parameter frames that match no grammar.
    pub(crate) unparsed: Vec<UnparsedParameterRecords>,
}

/// Decode every indexed parameter record in each Design `BulkStream`.
///
/// A frame whose tail names the stream's parameter collection and that matches
/// no parameter grammar is counted per stream so the caller reports it instead
/// of dropping it silently.
pub(crate) fn decode_parameters(scan: &ContainerScan) -> Result<DecodedParameters, CodecError> {
    let mut out = Vec::new();
    let mut unparsed = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let tail_targets = parameter_tail_targets(scan, &entry.name)?;
        let mut position = 0usize;
        let mut emitted_record_indices = HashSet::new();
        let mut unparsed_count = 0usize;
        while let Some(at) = next_indexed_record_offset(bytes, position) {
            let end = next_indexed_record_offset(bytes, at + 11).unwrap_or(bytes.len());
            let frame = &bytes[at..end];
            let grammar = parse_design_parameter(frame)?;
            let refused = grammar.is_some() && tail_targets.refuses(frame);
            if let Some(parsed) = grammar.filter(|_| !refused) {
                // The Design primary index exposes one live header for each
                // logical record index. Keep the first serialized parameter
                // frame so stale copies cannot create duplicate owner
                // bindings or duplicate neutral parameter identities.
                if emitted_record_indices.contains(&parsed.record_index) {
                    position = end;
                    continue;
                }

                let record_index = parsed.record_index;
                match locate_design_parameter(parsed, &entry.name, at) {
                    Some(parameter) => {
                        emitted_record_indices.insert(record_index);
                        out.push(parameter);
                    }
                    None => unparsed_count = next_unparsed_count(unparsed_count)?,
                }
                position = end;
            } else {
                if is_typed_parameter_frame(frame)
                    || tail_targets.marks_parameter_frame(frame)
                    || (refused && !tail_targets.collections.is_empty())
                {
                    unparsed_count = next_unparsed_count(unparsed_count)?;
                }
                position = at + 1;
            }
        }
        if unparsed_count != 0 {
            unparsed.push(UnparsedParameterRecords {
                stream: entry.name.clone(),
                count: unparsed_count,
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(DecodedParameters {
        parameters: out,
        unparsed,
    })
}

/// Design parameter parsed in frame-relative coordinates.
pub(in crate::design) struct ParsedDesignParameter {
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
    source_ordinal: u32,
    source_kind: String,
    owner_record_index: Option<u32>,
    family_discriminator: Option<crate::records::parameters::DesignParameterDiscriminator>,
    /// Frame-relative offset of the family discriminator.
    discriminator_offset: u64,
    expression: String,
    expression_offset: FrameRelative,
    source_kind_offset: FrameRelative,
    unit: Option<ParsedParameterUnit>,
    name: String,
    name_offset: FrameRelative,
    evaluated_value: f64,
    evaluated_value_offset: FrameRelative,
    flags: crate::records::parameters::DesignParameterFlags,
}

/// Unit token parsed in frame-relative coordinates.
struct ParsedParameterUnit {
    value: String,
    offset: FrameRelative,
}

impl ParsedDesignParameter {
    /// Locate this parameter in its containing stream.
    pub(in crate::design) fn into_record(
        self,
        stream: &str,
        frame_start: u64,
    ) -> Option<DesignParameter> {
        let family_discriminator = match self.family_discriminator {
            Some(value) => Some(crate::records::identity::Located {
                value,
                offset: FrameRelative(i128::from(self.discriminator_offset))
                    .absolute(frame_start)?,
            }),
            None => None,
        };
        let unit = match self.unit {
            Some(unit) => Some(crate::records::identity::RecordedValue {
                value: unit.value,
                offset: unit.offset.absolute(frame_start)?,
            }),
            None => None,
        };
        crate::records::parameters::DesignParameter::try_from(
            crate::records::parameters::DesignParameterDraft {
                id: ids::native_design_parameter_id(stream, frame_start),
                byte_offset: frame_start,
                class_tag: self.class_tag,
                record_index: self.record_index,
                source_ordinal: self.source_ordinal,
                source: crate::records::parameters::DesignParameterSource::new(
                    self.source_kind,
                    self.owner_record_index,
                    family_discriminator,
                )
                .ok()?,
                expression: self.expression,
                expression_offset: self.expression_offset.absolute(frame_start)?,
                source_kind_offset: self.source_kind_offset.absolute(frame_start)?,
                unit,
                name: self.name,
                name_offset: self.name_offset.absolute(frame_start)?,
                evaluated_value: self.evaluated_value,
                evaluated_value_offset: self.evaluated_value_offset.absolute(frame_start)?,
            },
        )
        .ok()?
        .with_flags(self.flags)
        .ok()
    }
}

/// Frame-relative offset of the parameter family discriminator.
const DESIGN_PARAMETER_DISCRIMINATOR_FRAME_OFFSET: usize = 22;
/// Further zero bytes before the discriminator in the wide discriminated form.
const WIDE_DISCRIMINATED_SHIFT: usize = 4;

/// Parse one indexed parameter frame in a Design `BulkStream` test fixture.
#[cfg(test)]
pub(in crate::design) fn parse_design_parameter_record(payload: &[u8]) -> Option<DesignParameter> {
    parse_design_parameter(payload)
        .unwrap()?
        .into_record(TEST_PARAMETER_STREAM, 0)
}

/// Design `BulkStream` name used when a test parses one isolated frame.
#[cfg(test)]
const TEST_PARAMETER_STREAM: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

/// Locate one parsed parameter frame in its containing stream. A frame that
/// closes the grammar with fields no parameter record admits yields `None`.
fn locate_design_parameter(
    parsed: ParsedDesignParameter,
    stream: &str,
    at: usize,
) -> Option<DesignParameter> {
    parsed.into_record(stream, u64::try_from(at).ok()?)
}

fn next_unparsed_count(count: usize) -> Result<usize, CodecError> {
    count.checked_add(1).ok_or_else(|| {
        CodecError::malformed("Fusion Design unparsed parameter count exceeds the platform range")
    })
}

pub(in crate::design) fn parse_design_parameter(
    payload: &[u8],
) -> Result<Option<ParsedDesignParameter>, CodecError> {
    let parsed = (|| {
        if View::u32_le_at(payload, 0) != Some(3) || payload.get(11..22) != Some(&[0; 11]) {
            return None;
        }
        let raw_tag = payload.get(4..7)?;
        if !raw_tag.iter().all(u8::is_ascii_graphic) {
            return None;
        }
        let class_tag = crate::records::references::DesignClassTag::try_from(
            std::str::from_utf8(raw_tag).ok()?.to_owned(),
        )
        .ok()?;
        let record_index = View::u32_le_at(payload, 7)?;
        if typed_parameter_tail_start(payload).is_some() {
            return match parse_typed_compact_design_parameter(payload, class_tag, record_index) {
                Ok(value) => value.map(Ok),
                Err(error) => Some(Err(error)),
            };
        }
        if class_tag.as_str() == "287" {
            match parse_legacy_287_design_parameter(payload, class_tag.clone(), record_index) {
                Ok(Some(value)) => return Some(Ok(value)),
                Err(error) => return Some(Err(error)),
                Ok(None) => {}
            }
        }
        let compact_owned = payload.get(11..26) == Some(&[0; 15])
            && payload.get(30) == Some(&1)
            && payload.get(35..41) == Some(&[0; 6]);
        let discriminated = !compact_owned && payload.get(30) == Some(&0);
        if !compact_owned && !discriminated {
            return match parse_legacy_design_parameter(payload, class_tag, record_index) {
                Ok(value) => value.map(Ok),
                Err(error) => Some(Err(error)),
            };
        }
        parse_header_form(payload, class_tag.clone(), record_index, discriminated, 0).or_else(
            || {
                wide_discriminated_parameter(payload).then(|| {
                    parse_header_form(
                        payload,
                        class_tag,
                        record_index,
                        true,
                        WIDE_DISCRIMINATED_SHIFT,
                    )
                })?
            },
        )
    })();
    parsed.transpose()
}

/// Index of the expression-trailer flag byte that marks a document parameter.
const DOCUMENT_TRAILER_FLAG: usize = 8;

/// Whether a discriminated expression trailer is four zero bytes followed by
/// five flag bytes that are each zero or one.
fn is_flagged_expression_trailer(trailer: &[u8]) -> bool {
    trailer.len() == DOCUMENT_TRAILER_FLAG + 1
        && trailer[..4] == [0; 4]
        && trailer[4..].iter().all(|byte| *byte <= 1)
}

fn trailer_flag(bytes: &[u8], at: usize) -> Option<bool> {
    match bytes.get(at)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// Flags and document flag of the wide expression trailer that starts `bytes`.
fn wide_expression_trailer_flags(
    bytes: &[u8],
) -> Option<(crate::records::parameters::DesignParameterFlags, bool)> {
    if bytes.get(wide_trailer::ZERO_RUN_6..wide_trailer::DRIVEN) != Some(&[0; 6][..])
        || bytes.get(wide_trailer::RESERVED_7) != Some(&wide_trailer::RESERVED_7_VALUE)
        || bytes.get(wide_trailer::RESERVED_9) != Some(&wide_trailer::RESERVED_9_VALUE)
    {
        return None;
    }
    let flags = crate::records::parameters::DesignParameterFlags {
        driven: trailer_flag(bytes, wide_trailer::DRIVEN)?,
        favorite: trailer_flag(bytes, wide_trailer::FAVORITE)?,
    };
    Some((flags, trailer_flag(bytes, wide_trailer::DOCUMENT)?))
}

/// Driven flag of a compact owned expression trailer.
fn compact_trailer_driven(trailer: &[u8]) -> Option<bool> {
    if trailer.len() != compact_trailer::LEN
        || trailer.get(compact_trailer::RESERVED_0) != Some(&compact_trailer::RESERVED_0_VALUE)
        || trailer.get(compact_trailer::ZERO_RUN_3..) != Some(&[0; 3][..])
    {
        return None;
    }
    trailer_flag(trailer, compact_trailer::DRIVEN)
}

/// Parse the discriminated or ordinary compact owned form. `shift` counts the
/// further zero bytes before the discriminator of the wide discriminated form.
fn parse_header_form(
    payload: &[u8],
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
    discriminated: bool,
    shift: usize,
) -> Option<Result<ParsedDesignParameter, CodecError>> {
    let discriminator_at = DESIGN_PARAMETER_DISCRIMINATOR_FRAME_OFFSET + shift;
    let (family_discriminator, source_ordinal, owner_record_index, expression_at, trailer_len) =
        if discriminated {
            let discriminator = crate::records::parameters::DesignParameterDiscriminator::try_from(
                View::u64_le_at(payload, discriminator_at)?,
            )
            .ok()?;
            let owner = match payload.get(35 + shift)? {
                0 => (None, 36 + shift, 9),
                1 if payload.get(40 + shift..46 + shift) == Some(&[0; 6]) => {
                    (Some(View::u32_le_at(payload, 36 + shift)?), 46 + shift, 9)
                }
                _ => return None,
            };
            (
                Some(discriminator),
                View::u32_le_at(payload, 31 + shift)?,
                owner.0,
                owner.1,
                owner.2,
            )
        } else {
            (
                None,
                View::u32_le_at(payload, 26)?,
                Some(View::u32_le_at(payload, 31)?),
                41,
                compact_trailer::LEN,
            )
        };
    let (expression, expression_end) = parameter_text!(payload, expression_at, 1..=256);
    let expression_trailer = payload.get(expression_end..expression_end + trailer_len)?;
    let long_document_trailer = discriminated
        && owner_record_index.is_none()
        && payload.get(expression_end..expression_end + 10)
            == Some(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
    let wide_flags = (shift == WIDE_DISCRIMINATED_SHIFT
        && family_discriminator.is_some_and(|value| matches!(value.code(), 0 | 6)))
    .then(|| wide_expression_trailer_flags(payload.get(expression_end..)?))
    .flatten()
    .filter(|(_, document)| *document == owner_record_index.is_none());
    let compact_driven = if discriminated {
        None
    } else {
        compact_trailer_driven(expression_trailer)
    };
    let valid_expression_trailer = if discriminated {
        long_document_trailer
            || wide_flags.is_some()
            || is_flagged_expression_trailer(expression_trailer)
    } else {
        compact_driven.is_some()
    };
    if !valid_expression_trailer {
        return None;
    }
    let prefixed_kind = if long_document_trailer
        || (discriminated
            && owner_record_index.is_some()
            && payload.get(expression_end..expression_end + 10) == Some(&[0; 10]))
    {
        lp_utf16_bounded(payload, expression_end + 10, 1..=256)
    } else {
        None
    };
    let extended_kind = if wide_flags.is_some() {
        lp_utf16_bounded(payload, expression_end + wide_trailer::LEN, 1..=256)
    } else {
        None
    };
    let wide_flags = wide_flags.filter(|_| extended_kind.is_some());
    let source_kind_at = if prefixed_kind.is_some() {
        expression_end + 10
    } else if extended_kind.is_some() {
        expression_end + wide_trailer::LEN
    } else {
        expression_end + trailer_len
    };
    let (source_kind, source_kind_end) = match prefixed_kind.or(extended_kind) {
        Some(value) => value,
        None => parameter_text!(payload, source_kind_at, 1..=256),
    };
    let document_flag = match wide_flags {
        Some((_, document)) => document,
        None => expression_trailer.get(DOCUMENT_TRAILER_FLAG) == Some(&1),
    };
    if discriminated
        && !long_document_trailer
        && document_flag != crate::records::parameters::is_document_source_kind(&source_kind)
    {
        return None;
    }
    let flags = match (wide_flags, compact_driven) {
        (Some((flags, _)), _) => flags,
        (None, Some(driven)) => crate::records::parameters::DesignParameterFlags {
            driven,
            favorite: false,
        },
        (None, None) => crate::records::parameters::DesignParameterFlags::default(),
    };
    let first_at = source_kind_end + usize::from(discriminated) * 4;
    if discriminated && View::u32_le_at(payload, source_kind_end) != Some(0) {
        return None;
    }
    let (unit, name, name_at, name_end) = if View::u32_le_at(payload, first_at) == Some(0) {
        let name_at = first_at + 4;
        let (name, name_end) = parameter_text!(payload, name_at, 1..=256);
        (None, name, name_at, name_end)
    } else {
        let (first, first_end) = parameter_text!(payload, first_at, 1..=256);
        let second_field = lp_utf16_bounded(payload, first_end, 1..=256);
        if let Some((second, second_end)) = second_field {
            (
                Some(ParsedParameterUnit {
                    value: first,
                    offset: FrameRelative(i128::from(u64_from_index(first_at + 4))),
                }),
                second,
                first_end,
                second_end,
            )
        } else {
            (None, first, first_at, first_end)
        }
    };
    let evaluated_value = View::f64_le_at(payload, name_end)?;
    if !is_parameter_tail(payload.get(name_end + 8..)?) || source_kind.is_empty() {
        return None;
    }
    Some(Ok(ParsedDesignParameter {
        class_tag,
        record_index,
        source_ordinal,
        source_kind,
        owner_record_index,
        family_discriminator,
        discriminator_offset: u64_from_index(discriminator_at),
        expression,
        expression_offset: FrameRelative(i128::from(u64_from_index(expression_at + 4))),
        source_kind_offset: FrameRelative(i128::from(u64_from_index(source_kind_at + 4))),
        unit,
        name,
        name_offset: FrameRelative(i128::from(u64_from_index(name_at + 4))),
        evaluated_value,
        evaluated_value_offset: FrameRelative(i128::from(u64_from_index(name_end))),
        flags,
    }))
}

/// Whether a discriminated parameter frame can store four further zero bytes
/// before its discriminator: offsets 11 through 25 are zero, a discriminator is
/// at offset 26, and offset 34 is zero. Discriminator 0 leaves offsets 22 through
/// 29 zero, which also read as a discriminator at offset 22.
fn wide_discriminated_parameter(payload: &[u8]) -> bool {
    payload.get(11..26) == Some(&[0; 15])
        && View::u64_le_at(
            payload,
            DESIGN_PARAMETER_DISCRIMINATOR_FRAME_OFFSET + WIDE_DISCRIMINATED_SHIFT,
        )
        .is_some_and(|value| {
            crate::records::parameters::DesignParameterDiscriminator::try_from(value).is_ok()
        })
        && payload.get(34) == Some(&0)
}

/// Parse the class-287 owned parameter family.
///
/// This family uses the compact-owned prefix. Its expression is followed by one
/// of the two fixed five-byte trailers.
fn parse_legacy_287_design_parameter(
    payload: &[u8],
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
) -> Result<Option<ParsedDesignParameter>, CodecError> {
    let parsed = (|| {
        if payload.get(legacy_287::ZERO_RUN_15..legacy_287::SOURCE_ORDINAL) != Some(&[0; 15])
            || payload.get(legacy_287::OWNER_MARKER) != Some(&legacy_287::OWNER_MARKER_VALUE)
            || payload.get(legacy_287::ZERO_RUN_6..legacy_287::EXPRESSION_LENGTH) != Some(&[0; 6])
        {
            return None;
        }
        let source_ordinal = View::u32_le_at(payload, legacy_287::SOURCE_ORDINAL)?;
        let owner_record_index = View::u32_le_at(payload, legacy_287::OWNER_RECORD_INDEX)?;
        let (expression, expression_end) =
            parameter_text!(payload, legacy_287::EXPRESSION_LENGTH, 1..=256);
        let expression_trailer_end =
            expression_end.checked_add(CLASS_287_EXPRESSION_TRAILER_LEN)?;
        let expression_trailer = payload.get(expression_end..expression_trailer_end)?;
        if !matches!(expression_trailer, [0, 0, 0, 0 | 1, 0]) {
            return None;
        }
        let source_kind_at = expression_trailer_end;
        let (source_kind, source_kind_end) = parameter_text!(payload, source_kind_at, 1..=256);
        let (unit, name, name_at, name_end) =
            if View::u32_le_at(payload, source_kind_end) == Some(0) {
                let name_at = source_kind_end.checked_add(4)?;
                let (name, name_end) = parameter_text!(payload, name_at, 1..=256);
                (None, name, name_at, name_end)
            } else {
                let (unit, unit_end) = parameter_text!(payload, source_kind_end, 1..=64);
                let (name, name_end) = parameter_text!(payload, unit_end, 1..=256);
                let unit_offset = source_kind_end.checked_add(4)?;
                (
                    Some(ParsedParameterUnit {
                        value: unit,
                        offset: FrameRelative(i128::try_from(unit_offset).ok()?),
                    }),
                    name,
                    unit_end,
                    name_end,
                )
            };
        let evaluated_value = View::f64_le_at(payload, name_end)?;
        let tail_start = name_end.checked_add(8)?;
        if !is_parameter_tail(payload.get(tail_start..)?) || source_kind.is_empty() {
            return None;
        }
        Some(Ok(ParsedDesignParameter {
            class_tag,
            record_index,
            source_ordinal,
            source_kind,
            owner_record_index: Some(owner_record_index),
            family_discriminator: None,
            discriminator_offset: 0,
            expression,
            expression_offset: FrameRelative(
                i128::try_from(legacy_287::EXPRESSION_LENGTH + 4).ok()?,
            ),
            source_kind_offset: FrameRelative(i128::try_from(source_kind_at.checked_add(4)?).ok()?),
            unit,
            name,
            name_offset: FrameRelative(i128::try_from(name_at.checked_add(4)?).ok()?),
            evaluated_value,
            evaluated_value_offset: FrameRelative(i128::try_from(name_end).ok()?),
            flags: crate::records::parameters::DesignParameterFlags::default(),
        }))
    })();
    parsed.transpose()
}

const CLASS_287_EXPRESSION_TRAILER_LEN: usize = 5;

/// Design type of a parameter collection, which every parameter tail names.
const PARAMETER_TAIL_TYPE_GUID: &str = "B0556F58-EE09-46A7-B18F-E296F48E5B13";

/// Records that the paired Design `MetaStream` registers for one `BulkStream`.
struct ParameterTailTargets {
    /// Records registered to the parameter-collection type.
    collections: HashSet<u64>,
    /// Records registered to every other type.
    others: HashSet<u64>,
}

impl ParameterTailTargets {
    /// Whether `frame` is shaped as a parameter record by its parameter tail.
    fn marks_parameter_frame(&self, frame: &[u8]) -> bool {
        parameter_tail_collection(frame).is_some_and(|target| {
            let target = u64::from(target);
            if self.collections.is_empty() {
                frame.get(indexed_header::LEN..DESIGN_PARAMETER_DISCRIMINATOR_FRAME_OFFSET)
                    == Some(&[0; 11])
                    && !self.others.contains(&target)
            } else {
                self.collections.contains(&target)
            }
        })
    }

    /// Whether the twelve-byte tail of `frame` names a record that cannot be
    /// its parameter collection.
    fn refuses(&self, frame: &[u8]) -> bool {
        parameter_tail_collection(frame).is_some_and(|target| {
            let target = u64::from(target);
            if self.collections.is_empty() {
                self.others.contains(&target)
            } else {
                !self.collections.contains(&target)
            }
        })
    }
}

fn parameter_tail_targets(
    scan: &ContainerScan,
    stream: &str,
) -> Result<ParameterTailTargets, CodecError> {
    let mut targets = ParameterTailTargets {
        collections: HashSet::new(),
        others: HashSet::new(),
    };
    if let Some(meta) = super::meta::metadata_for_bulk_stream(scan, stream)? {
        for ty in &meta.types {
            let records = if ty
                .type_guid
                .as_str()
                .eq_ignore_ascii_case(PARAMETER_TAIL_TYPE_GUID)
            {
                &mut targets.collections
            } else {
                &mut targets.others
            };
            records.extend(ty.entities.values().copied());
        }
    }
    Ok(targets)
}

/// The parameter collection named by the twelve-byte parameter tail that ends
/// `frame`.
fn parameter_tail_collection(frame: &[u8]) -> Option<u32> {
    let tail = frame.get(frame.len().checked_sub(parameter_tail::LEN)?..)?;
    if tail[parameter_tail::SEPARATOR] != parameter_tail::SEPARATOR_VALUE
        || tail[parameter_tail::COLLECTION_MARKER] != parameter_tail::COLLECTION_MARKER_VALUE
        || tail[parameter_tail::ZERO_RUN_6..]
            .iter()
            .any(|byte| *byte != 0)
    {
        return None;
    }
    View::u32_le_at(tail, parameter_tail::COLLECTION_RECORD_INDEX).filter(|target| *target != 0)
}

/// Whether `tail` is exactly one twelve-byte parameter tail.
fn is_parameter_tail(tail: &[u8]) -> bool {
    tail.len() == parameter_tail::LEN && parameter_tail_collection(tail).is_some()
}

/// Byte length of the typed parameter tail: `u8 0` plus a typed local reference.
const TYPED_PARAMETER_TAIL_LEN: usize = 52;

/// Expression offset after an owner reference without an inline type GUID.
const PLAIN_OWNER_EXPRESSION_LENGTH: usize = typed::OWNER_MARKER + 11;

/// Expression trailer of a typed compact owned parameter.
const TYPED_OWNED_EXPRESSION_TRAILER: [u8; 5] = [0; 5];

/// Expression trailer of a typed compact document parameter.
const TYPED_DOCUMENT_EXPRESSION_TRAILER: [u8; 5] = [0, 0, 0, 0, 1];

/// Start of the typed parameter tail when `payload` ends with one.
///
/// The tail is `u8 0` and a typed local reference whose target type GUID is
/// the parameter-collection type; it ends the frame exactly.
fn typed_parameter_tail_start(payload: &[u8]) -> Option<usize> {
    let start = payload.len().checked_sub(TYPED_PARAMETER_TAIL_LEN)?;
    if payload.get(start) != Some(&0) {
        return None;
    }
    let mut cursor = start + 1;
    let reference = crate::bytes::take_reference(payload, &mut cursor)?;
    let (_, guid) = reference.local()?;
    (guid == Some(PARAMETER_TAIL_TYPE_GUID) && cursor == payload.len()).then_some(start)
}

/// Whether `payload` is shaped as a typed compact parameter frame: the
/// compact fifteen-byte zero run and the typed parameter tail.
pub(in crate::design) fn is_typed_parameter_frame(payload: &[u8]) -> bool {
    payload.get(typed::ZERO_RUN_15..typed::SOURCE_ORDINAL) == Some(&[0; 15])
        && typed_parameter_tail_start(payload).is_some()
}

/// Parse the typed compact parameter form.
///
/// The owner field is a reference: a typed local reference for an owned
/// parameter or the null reference for a document parameter. The evaluated
/// scalar ends exactly where the typed parameter tail begins.
fn parse_typed_compact_design_parameter(
    payload: &[u8],
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
) -> Result<Option<ParsedDesignParameter>, CodecError> {
    let parsed = (|| {
        if !is_typed_parameter_frame(payload) {
            return None;
        }
        let tail_start = typed_parameter_tail_start(payload)?;
        let scalar_at = tail_start.checked_sub(8)?;
        let source_ordinal = View::u32_le_at(payload, typed::SOURCE_ORDINAL)?;
        let mut cursor = typed::OWNER_MARKER;
        let owner = crate::bytes::take_reference(payload, &mut cursor)?;
        let (owner_record_index, trailer) = match owner {
            crate::bytes::Reference::Null => (None, TYPED_DOCUMENT_EXPRESSION_TRAILER),
            crate::bytes::Reference::Local {
                target,
                inline_type_guid,
            } if (inline_type_guid.is_some()
                && cursor == typed::EXPRESSION_LENGTH
                && payload.get(typed::OWNER_TYPE_GUID_LENGTH..typed::OWNER_TYPE_GUID)
                    == Some(&typed::OWNER_TYPE_GUID_LENGTH_VALUE.to_le_bytes()[..]))
                || (inline_type_guid.is_none() && cursor == PLAIN_OWNER_EXPRESSION_LENGTH) =>
            {
                (
                    Some(u32::try_from(target).ok()?),
                    TYPED_OWNED_EXPRESSION_TRAILER,
                )
            }
            _ => return None,
        };
        let expression_at = cursor;
        let (expression, expression_end) = parameter_text!(payload, expression_at, 1..=256);
        let trailer_end = expression_end.checked_add(trailer.len())?;
        if payload.get(expression_end..trailer_end) != Some(&trailer[..]) {
            return None;
        }
        let source_kind_at = trailer_end;
        let (source_kind, source_kind_end) = parameter_text!(payload, source_kind_at, 1..=256);
        let (first, first_end) = if View::u32_le_at(payload, source_kind_end) == Some(0) {
            (None, source_kind_end.checked_add(4)?)
        } else {
            let (first, first_end) = parameter_text!(payload, source_kind_end, 1..=256);
            (Some(first), first_end)
        };
        let (unit, name, name_at, name_end) = match first {
            Some(first) if first_end == scalar_at => (None, first, source_kind_end, first_end),
            first => {
                let (name, name_end) = parameter_text!(payload, first_end, 1..=256);
                if name_end != scalar_at {
                    return None;
                }
                let unit = first.map(|value| ParsedParameterUnit {
                    value,
                    offset: FrameRelative(i128::from(u64_from_index(source_kind_end + 4))),
                });
                (unit, name, first_end, name_end)
            }
        };
        let evaluated_value = View::f64_le_at(payload, name_end)?;
        // The null owner reference denotes a document parameter; an ownerless
        // feature input or dimension has no admitted typed form.
        if source_kind.is_empty()
            || owner_record_index.is_none()
                != crate::records::parameters::is_document_source_kind(&source_kind)
        {
            return None;
        }
        Some(Ok(ParsedDesignParameter {
            class_tag,
            record_index,
            source_ordinal,
            source_kind,
            owner_record_index,
            family_discriminator: None,
            discriminator_offset: 0,
            expression,
            expression_offset: FrameRelative(i128::from(u64_from_index(expression_at + 4))),
            source_kind_offset: FrameRelative(i128::from(u64_from_index(source_kind_at + 4))),
            unit,
            name,
            name_offset: FrameRelative(i128::from(u64_from_index(name_at + 4))),
            evaluated_value,
            evaluated_value_offset: FrameRelative(i128::from(u64_from_index(name_end))),
            flags: crate::records::parameters::DesignParameterFlags::default(),
        }))
    })();
    parsed.transpose()
}

fn parse_legacy_design_parameter(
    payload: &[u8],
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
) -> Result<Option<ParsedDesignParameter>, CodecError> {
    let parsed = (|| {
        if payload.get(11..25)? != [0; 14]
            || payload.get(29) != Some(&1)
            || payload.get(34..40)? != [0; 6]
        {
            return None;
        }
        let source_ordinal = View::u32_le_at(payload, 25)?;
        let owner_record_index = View::u32_le_at(payload, 30)?;
        let expression_at = 40;
        let (expression, expression_end) = parameter_text!(payload, expression_at, 1..=256);
        if payload.get(expression_end..expression_end + 5)? != [0; 5] {
            return None;
        }
        let source_kind_at = expression_end + 5;
        let (source_kind, source_kind_end) = parameter_text!(payload, source_kind_at, 1..=256);
        let unit_at = source_kind_end;
        let (unit, unit_end) = parameter_text!(payload, unit_at, 1..=64);
        let name_at = unit_end;
        let (name, name_end) = parameter_text!(payload, name_at, 1..=256);
        let evaluated_value = View::f64_le_at(payload, name_end)?;
        if !is_parameter_tail(payload.get(name_end + 8..)?) || source_kind.is_empty() {
            return None;
        }
        Some(Ok(ParsedDesignParameter {
            class_tag,
            record_index,
            source_ordinal,
            source_kind,
            owner_record_index: Some(owner_record_index),
            family_discriminator: None,
            discriminator_offset: 0,
            expression,
            expression_offset: FrameRelative(i128::from(u64_from_index(expression_at + 4))),
            source_kind_offset: FrameRelative(i128::from(u64_from_index(source_kind_at + 4))),
            unit: Some(ParsedParameterUnit {
                value: unit,
                offset: FrameRelative(i128::from(u64_from_index(unit_at + 4))),
            }),
            name,
            name_offset: FrameRelative(i128::from(u64_from_index(name_at + 4))),
            evaluated_value,
            evaluated_value_offset: FrameRelative(i128::from(u64_from_index(name_end))),
            flags: crate::records::parameters::DesignParameterFlags::default(),
        }))
    })();
    parsed.transpose()
}

pub(crate) fn design_parameter_discriminator(source_kind: &str) -> u64 {
    match source_kind {
        "ScaleFactor" => 5,
        "TangencyWeight" => 6,
        _ => 0,
    }
}

/// Whether a class tag admits the legacy owner grammar without scope or scalar
/// lanes.
/// Design type of every parameter-owner record.
const PARAMETER_OWNER_TYPE_GUID: &str = "D91D429C-1BB5-4640-B49F-69AF14F463E9";

pub(crate) fn is_legacy_parameter_owner_68_class(class_tag: &str) -> bool {
    matches!(
        class_tag,
        "268" | "282" | "284" | "289" | "292" | "297" | "299" | "325" | "336"
    )
}

/// Whether a class tag admits the legacy owner grammar with repeated scope
/// references but without scalar or local-ordinal lanes.
pub(crate) fn is_legacy_parameter_owner_88_class(class_tag: &str) -> bool {
    matches!(class_tag, "284" | "282" | "336" | "325" | "297")
}

/// Decoded owner bindings and parameters with an unsupported owner frame.
#[derive(Debug, Default)]
pub(crate) struct DecodedParameterOwners {
    /// Owner frames that satisfy an admitted grammar.
    pub(crate) owners: Vec<DesignParameterOwner>,
    /// Native identities of parameters with an unsupported owner frame.
    pub(crate) unparsed: Vec<String>,
}

/// Keep parameters with unsupported owner frames in native storage only.
pub(crate) fn parameters_for_projection<'a>(
    parameters: &'a [DesignParameter],
    unparsed_owners: &[String],
) -> Cow<'a, [DesignParameter]> {
    if unparsed_owners.is_empty() {
        return Cow::Borrowed(parameters);
    }
    let unparsed: HashSet<&str> = unparsed_owners.iter().map(String::as_str).collect();
    Cow::Owned(
        parameters
            .iter()
            .filter(|parameter| !unparsed.contains(parameter.id.as_str()))
            .cloned()
            .collect(),
    )
}

/// Decode the exact same-index-delimited owner frame for every owned Design
/// parameter. Retain an unsupported binding without an interpreted owner.
pub(crate) fn decode_parameter_owners(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    headers: &[DesignRecordHeader],
) -> Result<DecodedParameterOwners, CodecError> {
    if parameters
        .iter()
        .all(|parameter| parameter.owner_record_index().is_none())
        && !has_pipe_type(scan)?
    {
        return Ok(DecodedParameterOwners::default());
    }
    let mut headers_by_stream = HashMap::<&str, HashMap<u32, &DesignRecordHeader>>::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let stream_headers = headers_by_stream.entry(stream).or_default();
        if stream_headers.insert(header.record_index, header).is_some() {
            return Err(crate::design::text::malformed_design(format_args!(
                "Fusion Design stream has duplicate primary headers for record {}",
                header.record_index
            )));
        }
    }
    let mut streams = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let stream = crate::ids::native_scope(&entry.name);
        if streams.contains_key(&stream) {
            return Err(CodecError::Malformed(
                "F3D contains duplicate Design BulkStream identities".into(),
            ));
        }

        // Primary frames whose MetaStream type is the parameter-owner type.
        let owner_typed = super::meta::metadata_for_bulk_stream(scan, &entry.name)?
            .and_then(|meta| {
                super::meta::design_primary_frames(bytes, &meta)
                    .ok()
                    .map(|frames| {
                        frames
                            .iter()
                            .filter(|frame| {
                                frame
                                    .design_type
                                    .type_guid
                                    .as_str()
                                    .eq_ignore_ascii_case(PARAMETER_OWNER_TYPE_GUID)
                            })
                            .map(|frame| frame.start)
                            .collect::<HashSet<_>>()
                    })
            })
            .unwrap_or_default();
        streams
            .entry(stream)
            .or_insert((entry, IndexedRecordOffsets::build(bytes), owner_typed));
    }
    let mut out = Vec::new();
    // Pipe scalar owners are canonical records of their scope; they are read
    // from their registered type even when no parameter links to them.
    let mut emitted = HashMap::new();
    for (stream, (entry, records, _)) in &streams {
        for owner in pipe_scalar_owners(scan, &entry.name, records)? {
            let key = (stream.clone(), owner.record_index());
            if let std::collections::hash_map::Entry::Vacant(slot) = emitted.entry(key) {
                slot.insert(out.len());
                out.push(owner);
            }
        }
    }
    let mut unparsed = Vec::new();
    for parameter in parameters {
        let Some(owner_index) = parameter.owner_record_index() else {
            continue;
        };
        let malformed = |invariant: &str| {
            crate::design::text::malformed_design(format_args!(
                "Fusion Design parameter {} owner {} {invariant}",
                parameter.record_index, owner_index
            ))
        };
        let scope = native_stream(&parameter.id)
            .ok_or_else(|| malformed("has no Design stream identity"))?;
        if let Some(ordinal) = emitted.get(&(scope.to_owned(), owner_index)).copied() {
            if out[ordinal].parameter_record_index() != parameter.record_index {
                return Err(malformed("does not link back to its referencing parameter"));
            }
            continue;
        }
        let Some(header) = headers_by_stream
            .get(scope)
            .and_then(|headers| headers.get(&owner_index))
            .copied()
        else {
            // A parameter can retain a source owner reference after Fusion has
            // omitted that owner's primary frame. Keep the parameter native;
            // projection reports the unresolved binding as a loss.
            continue;
        };
        let (entry, records, owner_typed) = streams
            .get(scope)
            .ok_or_else(|| malformed("has no containing Design BulkStream"))?;
        let bytes = scan.entry_bytes(&entry.name)?;
        let at = usize::try_from(header.byte_offset)
            .map_err(|_| malformed("primary header offset exceeds the platform address space"))?;
        if records.frames(owner_index).next().is_none() {
            // The lone record is the paired record of an omitted owner frame.
            continue;
        }
        let end = records
            .frames(owner_index)
            .find_map(|(start, end)| (start == at).then_some(end))
            .ok_or_else(|| malformed("has no following same-index paired header"))?;
        let frame = bytes
            .get(at..end)
            .ok_or_else(|| malformed("frame lies outside its Design BulkStream"))?;
        let evaluated = crate::records::identity::Located {
            value: parameter.evaluated_value().get(),
            offset: parameter.evaluated_value_offset(),
        };
        let parsed = parse_parameter_owner(frame)
            .or_else(|| {
                parse_admitted_legacy_parameter_owner_68(
                    frame,
                    evaluated,
                    header.byte_offset,
                    owner_typed.contains(&at),
                )
            })
            .or_else(|| {
                parse_admitted_legacy_parameter_owner_88(
                    frame,
                    evaluated,
                    header.byte_offset,
                    owner_typed.contains(&at),
                )
            });
        let Some(parsed) = parsed else {
            unparsed.push(parameter.id.clone());
            continue;
        };
        let owner = parsed
            .into_record(&entry.name, header.byte_offset)
            .ok_or_else(|| malformed("has invalid owner fields or evaluated-value offset"))?;
        if owner.record_index() != owner_index
            || owner.parameter_record_index() != parameter.record_index
        {
            return Err(malformed("does not link back to its referencing parameter"));
        }

        out.push(owner);
    }
    out.sort_by(|a, b| a.id().cmp(b.id()));
    Ok(DecodedParameterOwners {
        owners: out,
        unparsed,
    })
}

/// Whether any Design `MetaStream` registers the `Pipe` type.
fn has_pipe_type(scan: &ContainerScan) -> Result<bool, CodecError> {
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        if super::meta::metadata_for_bulk_stream(scan, &entry.name)?.is_some_and(|meta| {
            meta.types
                .iter()
                .any(crate::records::feature::path_features::pipe::is_pipe)
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Scalar owners of uniquely registered `Pipe` scopes in one Design stream.
///
/// An owner frame is admitted when its entity has one type registration, it
/// parses as a 103- or 104-byte parameter owner, it is the only indexed frame
/// with that extent, and its owning scope is a uniquely registered Pipe with
/// one primary frame.
fn pipe_scalar_owners(
    scan: &ContainerScan,
    entry_name: &str,
    records: &IndexedRecordOffsets,
) -> Result<Vec<DesignParameterOwner>, CodecError> {
    use crate::records::feature::path_features::pipe;
    let Some(meta) = super::meta::metadata_for_bulk_stream(scan, entry_name)? else {
        return Ok(Vec::new());
    };
    if !meta.types.iter().any(pipe::is_pipe) {
        return Ok(Vec::new());
    }
    let bytes = scan.entry_bytes(entry_name)?;
    let Ok(frames) = super::meta::design_primary_frames(bytes, &meta) else {
        return Ok(Vec::new());
    };
    let mut registrations = HashMap::<u64, usize>::new();
    for data in &meta.types {
        for id in data.entities.values() {
            *registrations.entry(*id).or_default() += 1;
        }
    }
    let unique = |id: u64| registrations.get(&id) == Some(&1);
    let mut pipe_frames = HashMap::<u64, usize>::new();
    for frame in frames
        .iter()
        .filter(|frame| pipe::is_pipe(frame.design_type) && unique(frame.entity_id))
    {
        *pipe_frames.entry(frame.entity_id).or_default() += 1;
    }
    let mut out = Vec::new();
    for frame in frames
        .iter()
        .filter(|frame| pipe::is_scalar(frame.design_type) && unique(frame.entity_id))
    {
        let Some(parsed) = bytes
            .get(frame.start..frame.member_end)
            .and_then(parse_parameter_owner)
        else {
            continue;
        };
        if !matches!(parsed.frame_length, 103 | 104)
            || pipe_frames.get(&u64::from(parsed.scope_record_index)) != Some(&1)
            || records
                .frames(parsed.record_index)
                .filter(|(start, end)| *start == frame.start && *end == frame.member_end)
                .count()
                != 1
        {
            continue;
        }
        if let Some(owner) = parsed.into_record(entry_name, u64_from_index(frame.start)) {
            out.push(owner);
        }
    }
    Ok(out)
}

/// Byte offset measured from an indexed frame start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FrameRelative(pub(crate) i128);

impl FrameRelative {
    fn between(offset: u64, frame_start: u64) -> Self {
        Self(i128::from(offset) - i128::from(frame_start))
    }

    /// Convert this offset to a stream-absolute position.
    pub(super) fn absolute(self, frame_start: u64) -> Option<u64> {
        u64::try_from(i128::from(frame_start).checked_add(self.0)?).ok()
    }
}

/// Parameter owner parsed in frame-relative coordinates.
pub(in crate::design) struct ParsedParameterOwner {
    pub(super) frame_length: u64,
    pub(super) class_tag: crate::records::references::DesignClassTag,
    pub(super) record_index: u32,
    pub(super) scope_record_index: u32,
    pub(super) local_ordinal: u32,
    pub(super) evaluated_value: FiniteReal,
    pub(super) evaluated_value_offset: FrameRelative,
    pub(super) parameter_record_index: u32,
    pub(super) owned_ordinal: u32,
    variant: Option<u8>,
    pub(super) companion_record_index: u32,
}

impl ParsedParameterOwner {
    /// Locate this owner in its containing stream.
    pub(in crate::design) fn into_record(
        self,
        stream: &str,
        frame_start: u64,
    ) -> Option<DesignParameterOwner> {
        crate::records::parameters::DesignParameterOwner::from_parts(
            crate::records::parameters::DesignParameterOwnerWire {
                id: ids::native_design_parameter_owner_id(stream, frame_start),
                byte_offset: frame_start,
                frame_length: self.frame_length,
                class_tag: self.class_tag,
                record_index: self.record_index,
                scope_record_index: self.scope_record_index,
                local_ordinal: self.local_ordinal,
                evaluated_value: self.evaluated_value,
                evaluated_value_offset: self.evaluated_value_offset.absolute(frame_start)?,
                parameter_record_index: self.parameter_record_index,
                owned_ordinal: self.owned_ordinal,
                variant: self.variant,
                companion_record_index: self.companion_record_index,
            },
        )
        .ok()
    }
}

pub(in crate::design) fn parse_parameter_owner(frame: &[u8]) -> Option<ParsedParameterOwner> {
    parse_parameter_owner_with(frame, OwnerReferenceForm::Plain)
        .or_else(|| parse_parameter_owner_with(frame, OwnerReferenceForm::Typed))
}

/// Same-segment reference spelling inside an ordinary parameter-owner frame.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnerReferenceForm {
    /// `01 + u64 target + 00 00`: eleven bytes.
    Plain,
    /// `01 + u64 target + LP-ASCII 36 type GUID + 00 00`: 51 bytes.
    Typed,
}

impl OwnerReferenceForm {
    /// Byte width of one non-null local reference in this spelling.
    const fn width(self) -> usize {
        match self {
            Self::Plain => 11,
            Self::Typed => 51,
        }
    }

    /// The u32 target of the local reference that occupies exactly
    /// `frame[at..at + width]`.
    fn target_at(self, frame: &[u8], at: usize) -> Option<u32> {
        let mut cursor = at;
        let reference = crate::bytes::take_reference(frame, &mut cursor)?;
        let (target, guid) = reference.local()?;
        let typed = guid.is_some();
        (cursor == at.checked_add(self.width())? && typed == (self == Self::Typed))
            .then(|| u32::try_from(target).ok())
            .flatten()
    }
}

// The plain spelling reproduces the fixed owner-prefix table: the scope
// reference's u32 target and six-byte high-half-and-flags run, then the local
// ordinal and the scalar envelope.
const _: () = {
    assert!(owner_prefix::SCOPE_RECORD_INDEX == owner_prefix::SCOPE_MARKER + 1);
    assert!(owner_prefix::ZERO_RUN_6 == owner_prefix::SCOPE_RECORD_INDEX + 4);
    assert!(
        owner_prefix::LOCAL_ORDINAL
            == owner_prefix::SCOPE_MARKER + OwnerReferenceForm::Plain.width()
    );
    assert!(owner_prefix::LEN == owner_prefix::LOCAL_ORDINAL + 4);
};

/// Parse the ordinary parameter-owner grammar with one reference spelling.
///
/// Every reference field has the same width within one frame, so the fixed
/// suffix is parsed backward from the exact paired-header boundary.
fn parse_parameter_owner_with(
    frame: &[u8],
    form: OwnerReferenceForm,
) -> Option<ParsedParameterOwner> {
    let width = form.width();
    let (class_tag, after_tag) = lp_ascii_filtered_view(frame, 0, 0..=2000, u8::is_ascii_graphic)?;
    let class_tag = crate::design::decode::text::class_tag_from_view(class_tag).ok()?;
    if after_tag != indexed_header::RECORD_INDEX
        || frame.get(owner_prefix::ZERO_RUN_8..owner_prefix::ONE_MARKER) != Some(&[0; 8])
        || frame.get(owner_prefix::ONE_MARKER..owner_prefix::SCOPE_MARKER) != Some(&[1, 1, 0, 0, 0])
    {
        return None;
    }
    let record_index = View::u32_le_at(frame, indexed_header::RECORD_INDEX)?;
    let scope_record_index = form.target_at(frame, owner_prefix::SCOPE_MARKER)?;
    let local_ordinal_offset = owner_prefix::SCOPE_MARKER + width;
    let scalar_start = local_ordinal_offset + 4;

    // Parse the fixed suffix backward from the exact paired-header boundary.
    // This prevents a valid shorter prefix from being accepted as the record.
    let final_scope_marker = frame.len().checked_sub(width)?;
    let companion_marker = final_scope_marker.checked_sub(width + 1)?;
    if form.target_at(frame, final_scope_marker) != Some(scope_record_index)
        || frame.get(companion_marker + width) != Some(&0)
    {
        return None;
    }
    let companion_record_index = form.target_at(frame, companion_marker)?;

    let repeated_scope = |at: usize| form.target_at(frame, at) == Some(scope_record_index);
    let without_variant = companion_marker.checked_sub(width + 2).and_then(|at| {
        (repeated_scope(at) && frame.get(at + width..companion_marker) == Some(&[0; 2]))
            .then_some((at, None))
    });
    let with_variant = companion_marker.checked_sub(width + 3).and_then(|at| {
        let variant = *frame.get(at + width + 1)?;
        (repeated_scope(at)
            && frame.get(at + width) == Some(&1)
            && variant <= 1
            && frame.get(at + width + 2).is_some_and(|flag| *flag <= 1))
        .then_some((at, Some(variant)))
    });
    let with_compact_variant = companion_marker.checked_sub(width + 2).and_then(|at| {
        let variant = *frame.get(at + width + 1)?;
        (repeated_scope(at) && frame.get(at + width) == Some(&1) && variant <= 1)
            .then_some((at, Some(variant)))
    });
    if [without_variant, with_variant, with_compact_variant]
        .into_iter()
        .flatten()
        .count()
        != 1
    {
        return None;
    }
    let (repeated_scope_marker, variant) =
        without_variant.or(with_variant).or(with_compact_variant)?;

    let owned_ordinal_offset = repeated_scope_marker.checked_sub(8)?;
    let parameter_marker = owned_ordinal_offset.checked_sub(width)?;
    if frame.get(owned_ordinal_offset + 4..repeated_scope_marker) != Some(&[0; 4]) {
        return None;
    }
    let parameter_record_index = form.target_at(frame, parameter_marker)?;

    let scalar = frame.get(scalar_start..parameter_marker)?;
    let (evaluated_value, evaluated_value_offset) = match scalar.len() {
        9 if scalar.first() == Some(&0) => {
            (View::f64_le_at(frame, scalar_start + 1)?, scalar_start + 1)
        }
        6 if matches!(scalar.get(..2), Some([0, 0 | 1])) => (
            f64::from(View::u32_le_at(frame, scalar_start + 2)?),
            scalar_start + 2,
        ),
        5 if scalar.first() == Some(&0) && variant.is_none() => (
            f64::from(View::u32_le_at(frame, scalar_start + 1)?),
            scalar_start + 1,
        ),
        13 if scalar.first() == Some(&1) && scalar.get(1..5) == Some(&[0; 4]) => {
            (View::f64_le_at(frame, scalar_start + 5)?, scalar_start + 5)
        }
        _ => return None,
    };
    let evaluated_value = FiniteReal::new(evaluated_value)?;
    let consecutive = |first: u32, second: u32, third: u32| {
        first.checked_add(1) == Some(second) && second.checked_add(1) == Some(third)
    };
    if !(consecutive(record_index, parameter_record_index, companion_record_index)
        || consecutive(parameter_record_index, record_index, companion_record_index)
        || consecutive(record_index, companion_record_index, parameter_record_index))
    {
        return None;
    }

    Some(ParsedParameterOwner {
        frame_length: u64::try_from(frame.len()).ok()?,
        class_tag,
        record_index,
        scope_record_index,
        local_ordinal: View::u32_le_at(frame, local_ordinal_offset)?,
        evaluated_value,
        evaluated_value_offset: FrameRelative(i128::from(u64_from_index(evaluated_value_offset))),
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, owned_ordinal_offset)?,
        variant,
        companion_record_index,
    })
}

/// Parse the legacy owner envelope whose scope and scalar lanes are absent.
///
/// The class admission is intentional. A short frame is not enough to select
/// this grammar because older class tags also occur on modern owner records.
#[cfg(test)]
fn parse_legacy_parameter_owner_68(
    frame: &[u8],
    evaluated: crate::records::identity::Located<f64>,
    frame_start: u64,
) -> Option<ParsedParameterOwner> {
    parse_admitted_legacy_parameter_owner_68(frame, evaluated, frame_start, false)
}

/// Whether the scalar lane of a legacy 68-byte owner is zero or stores the
/// linked parameter's evaluated scalar.
fn legacy_owner_68_scalar_lane(frame: &[u8], evaluated: f64) -> bool {
    let Some(lane) = frame.get(legacy_owner_68::SCALAR_MARKER..legacy_owner_68::PARAMETER_MARKER)
    else {
        return false;
    };
    lane == [0; 9]
        || (lane[0] == legacy_owner_68::SCALAR_MARKER_VALUE
            && View::f64_le_at(frame, legacy_owner_68::SCALAR)
                .is_some_and(|scalar| scalar.to_bits() == evaluated.to_bits()))
}

/// The legacy 68-byte owner grammar, admitted when the frame has the
/// parameter-owner type (`owner_typed`) or one of the listed class tags.
fn parse_admitted_legacy_parameter_owner_68(
    frame: &[u8],
    evaluated: crate::records::identity::Located<f64>,
    frame_start: u64,
    owner_typed: bool,
) -> Option<ParsedParameterOwner> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(frame, 0, 0..=2000, u8::is_ascii_graphic)?;
    if !(owner_typed || is_legacy_parameter_owner_68_class(class_tag))
        || frame.len() != legacy_owner_68::LEN
        || after_tag != indexed_header::RECORD_INDEX
        || frame.get(legacy_owner_68::ZERO_RUN_8..legacy_owner_68::FIRST_MARKER) != Some(&[0; 8])
        || frame.get(legacy_owner_68::FIRST_MARKER) != Some(&1)
        || frame.get(legacy_owner_68::ZERO_RUN_4..legacy_owner_68::SCALAR_MARKER) != Some(&[0; 4])
        || !legacy_owner_68_scalar_lane(frame, evaluated.value)
        || frame.get(legacy_owner_68::PARAMETER_MARKER) != Some(&1)
        || frame.get(legacy_owner_68::ZERO_RUN_6..legacy_owner_68::OWNED_ORDINAL) != Some(&[0; 6])
        || frame.get(legacy_owner_68::ZERO_RUN_5..legacy_owner_68::FIRST_FLAG) != Some(&[0; 5])
        || frame
            .get(legacy_owner_68::FIRST_FLAG)
            .is_none_or(|flag| *flag > 1)
        || frame
            .get(legacy_owner_68::SECOND_FLAG)
            .is_none_or(|flag| *flag > 1)
        || frame.get(legacy_owner_68::COMPANION_MARKER) != Some(&1)
        || frame.get(legacy_owner_68::ZERO_RUN_8_TAIL..legacy_owner_68::LEN) != Some(&[0; 8])
    {
        return None;
    }
    let record_index = View::u32_le_at(frame, indexed_header::RECORD_INDEX)?;
    let parameter_record_index = View::u32_le_at(frame, legacy_owner_68::PARAMETER_RECORD_INDEX)?;
    let companion_record_index = View::u32_le_at(frame, legacy_owner_68::COMPANION_RECORD_INDEX)?;
    let consecutive = |first: u32, second: u32, third: u32| {
        first.checked_add(1) == Some(second) && second.checked_add(1) == Some(third)
    };
    let evaluated_value = FiniteReal::new(evaluated.value)?;
    if !consecutive(record_index, parameter_record_index, companion_record_index) {
        return None;
    }
    Some(ParsedParameterOwner {
        frame_length: u64::try_from(legacy_owner_68::LEN).ok()?,
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        record_index,
        scope_record_index: 0,
        local_ordinal: 0,
        evaluated_value,
        evaluated_value_offset: FrameRelative::between(evaluated.offset, frame_start),
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, legacy_owner_68::OWNED_ORDINAL)?,
        variant: None,
        companion_record_index,
    })
}

/// Parse the legacy owner envelope whose scope is repeated in the suffix but
/// whose scalar and local-ordinal lanes are absent.
#[cfg(test)]
fn parse_legacy_parameter_owner_88(
    frame: &[u8],
    evaluated: crate::records::identity::Located<f64>,
    frame_start: u64,
) -> Option<ParsedParameterOwner> {
    parse_admitted_legacy_parameter_owner_88(frame, evaluated, frame_start, false)
}

/// The legacy 88-byte owner grammar, admitted when the frame has the
/// parameter-owner type (`owner_typed`) or one of the listed class tags.
fn parse_admitted_legacy_parameter_owner_88(
    frame: &[u8],
    evaluated: crate::records::identity::Located<f64>,
    frame_start: u64,
    owner_typed: bool,
) -> Option<ParsedParameterOwner> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(frame, 0, 0..=2000, u8::is_ascii_graphic)?;
    if !(owner_typed || is_legacy_parameter_owner_88_class(class_tag))
        || frame.len() != legacy_owner_88::LEN
        || after_tag != indexed_header::RECORD_INDEX
        || frame.get(legacy_owner_88::ZERO_RUN_8..legacy_owner_88::FIRST_MARKER) != Some(&[0; 8])
        || frame.get(legacy_owner_88::FIRST_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_13..legacy_owner_88::PARAMETER_MARKER)
            != Some(&[0; 13])
        || frame.get(legacy_owner_88::PARAMETER_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_6..legacy_owner_88::OWNED_ORDINAL) != Some(&[0; 6])
        || frame.get(legacy_owner_88::ZERO_RUN_4..legacy_owner_88::SCOPE_MARKER) != Some(&[0; 4])
        || frame.get(legacy_owner_88::SCOPE_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_8_BETWEEN_SCOPES..legacy_owner_88::COMPANION_MARKER)
            != Some(&[0; 8])
        || frame.get(legacy_owner_88::COMPANION_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_7..legacy_owner_88::REPEATED_SCOPE_MARKER)
            != Some(&[0; 7])
        || frame.get(legacy_owner_88::REPEATED_SCOPE_MARKER) != Some(&1)
        || frame.get(legacy_owner_88::ZERO_RUN_6_TAIL..legacy_owner_88::LEN) != Some(&[0; 6])
    {
        return None;
    }
    let record_index = View::u32_le_at(frame, indexed_header::RECORD_INDEX)?;
    let parameter_record_index = View::u32_le_at(frame, legacy_owner_88::PARAMETER_RECORD_INDEX)?;
    let scope_record_index = View::u32_le_at(frame, legacy_owner_88::SCOPE_RECORD_INDEX)?;
    if scope_record_index == 0
        || View::u32_le_at(frame, legacy_owner_88::REPEATED_SCOPE_RECORD_INDEX)?
            != scope_record_index
    {
        return None;
    }
    let companion_record_index = View::u32_le_at(frame, legacy_owner_88::COMPANION_RECORD_INDEX)?;
    let consecutive = |first: u32, second: u32, third: u32| {
        first.checked_add(1) == Some(second) && second.checked_add(1) == Some(third)
    };
    let evaluated_value = FiniteReal::new(evaluated.value)?;
    if !consecutive(record_index, parameter_record_index, companion_record_index) {
        return None;
    }
    Some(ParsedParameterOwner {
        frame_length: u64::try_from(legacy_owner_88::LEN).ok()?,
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        record_index,
        scope_record_index,
        local_ordinal: 0,
        evaluated_value,
        evaluated_value_offset: FrameRelative::between(evaluated.offset, frame_start),
        parameter_record_index,
        owned_ordinal: View::u32_le_at(frame, legacy_owner_88::OWNED_ORDINAL)?,
        variant: None,
        companion_record_index,
    })
}

/// Decode the fixed prefix of every indexed record paired with a parameter
/// owner. Record-specific payload after the prefix is decoded independently.
pub(crate) fn decode_parameter_companions(
    scan: &ContainerScan,
    owners: &[DesignParameterOwner],
    headers: &[DesignRecordHeader],
) -> Result<Vec<DesignParameterCompanion>, CodecError> {
    let mut headers_by_record = HashMap::new();
    for header in headers {
        let Some(stream) = native_stream(&header.id) else {
            continue;
        };
        let key = (stream, header.record_index);
        headers_by_record.insert(key, header);
    }
    let mut out = Vec::new();
    for owner in owners {
        let Some(scope) = native_stream(owner.id()) else {
            continue;
        };
        let Some(header) = headers_by_record.get(&(scope, owner.companion_record_index())) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, scope)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let at = usize::try_from(header.byte_offset).ok();
        let Some(parsed) = at
            .and_then(|at| bytes.get(at..))
            .and_then(parse_parameter_companion)
        else {
            continue;
        };
        if parsed.record_index != owner.companion_record_index()
            || parsed.owner_record_index != owner.record_index()
        {
            continue;
        }
        let Some(companion) = parsed.into_record(&entry.name, header.byte_offset) else {
            continue;
        };

        out.push(companion);
    }
    out.sort_by(|a, b| a.id().cmp(b.id()));
    Ok(out)
}

/// Parameter companion prefix parsed in frame-relative coordinates.
struct ParsedParameterCompanion {
    class_tag: crate::records::references::DesignClassTag,
    record_index: u32,
    owner_record_index: u32,
    timestamp_micros: std::num::NonZeroU64,
    timestamp_micros_offset: FrameRelative,
}

impl ParsedParameterCompanion {
    /// Locate this companion prefix in its containing stream. The owned payload
    /// extent and recipes are bound afterward by
    /// `bind_parameter_companion_payloads`.
    fn into_record(self, stream: &str, frame_start: u64) -> Option<DesignParameterCompanion> {
        let timestamp_offset = self.timestamp_micros_offset.absolute(frame_start)?;
        let id = design_record_id(stream, ":design-parameter-companion#", frame_start);
        Some(DesignParameterCompanion::unbound(
            id,
            frame_start,
            self.class_tag,
            self.record_index,
            self.owner_record_index,
            self.timestamp_micros,
            timestamp_offset,
        ))
    }
}

/// Parse the companion prefix at the start of `frame` in either reference
/// spelling.
fn parse_parameter_companion(frame: &[u8]) -> Option<ParsedParameterCompanion> {
    parse_plain_parameter_companion(frame.get(..companion_prefix::LEN)?)
        .or_else(|| parse_typed_parameter_companion(frame.get(..typed_companion::LEN)?))
}

/// Parse the typed-reference companion prefix: the indexed header, twenty
/// zero bytes, a typed owner reference, the nonzero timestamp, and eight
/// zero bytes.
fn parse_typed_parameter_companion(prefix: &[u8]) -> Option<ParsedParameterCompanion> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(prefix, 0, 0..=2000, u8::is_ascii_graphic)?;
    let class_tag = crate::design::decode::text::class_tag_from_view(class_tag).ok()?;
    let timestamp_at = typed_companion::TIMESTAMP_MICROS;
    if prefix.len() != typed_companion::LEN
        || after_tag != indexed_header::RECORD_INDEX
        || prefix.get(typed_companion::ZERO_RUN_20..typed_companion::OWNER_REFERENCE)
            != Some(&[0; 20])
        || prefix.get(typed_companion::ZERO_RUN_8..typed_companion::LEN) != Some(&[0; 8])
    {
        return None;
    }
    let owner_record_index =
        crate::bytes::typed_local_reference_target(prefix, typed_companion::OWNER_REFERENCE)?;
    let timestamp_micros = std::num::NonZeroU64::new(View::u64_le_at(prefix, timestamp_at)?)?;
    Some(ParsedParameterCompanion {
        class_tag,
        record_index: View::u32_le_at(prefix, indexed_header::RECORD_INDEX)?,
        owner_record_index,
        timestamp_micros,
        timestamp_micros_offset: FrameRelative(i128::from(u64_from_index(timestamp_at))),
    })
}

fn parse_plain_parameter_companion(prefix: &[u8]) -> Option<ParsedParameterCompanion> {
    let (class_tag, after_tag) = lp_ascii_filtered_view(prefix, 0, 0..=2000, u8::is_ascii_graphic)?;
    let class_tag = crate::design::decode::text::class_tag_from_view(class_tag).ok()?;
    if prefix.len() != companion_prefix::LEN
        || after_tag != indexed_header::RECORD_INDEX
        || prefix.get(companion_prefix::ZERO_RUN_20..companion_prefix::OWNER_MARKER)
            != Some(&[0; 20])
        || prefix.get(companion_prefix::OWNER_MARKER) != Some(&1)
        || prefix.get(companion_prefix::ZERO_RUN_6..companion_prefix::TIMESTAMP_MICROS)
            != Some(&[0; 6])
        || prefix.get(companion_prefix::ZERO_RUN_8..companion_prefix::LEN) != Some(&[0; 8])
    {
        return None;
    }
    let timestamp_micros =
        std::num::NonZeroU64::new(View::u64_le_at(prefix, companion_prefix::TIMESTAMP_MICROS)?)?;
    Some(ParsedParameterCompanion {
        class_tag,
        record_index: View::u32_le_at(prefix, indexed_header::RECORD_INDEX)?,
        owner_record_index: View::u32_le_at(prefix, companion_prefix::OWNER_RECORD_INDEX)?,
        timestamp_micros,
        timestamp_micros_offset: FrameRelative(i128::from(u64_from_index(
            companion_prefix::TIMESTAMP_MICROS,
        ))),
    })
}

/// Records a companion payload is resolved against.
pub(crate) struct ParameterCompanionInputs<'a, S: std::hash::BuildHasher> {
    /// Indexed parameter records.
    pub(crate) parameters: &'a [DesignParameter],
    /// Parameter owner frames.
    pub(crate) owners: &'a [DesignParameterOwner],
    /// Parameter scope records.
    pub(crate) scopes: &'a [DesignParameterScope],
    /// Design entity headers.
    pub(crate) entities: &'a [DesignEntityHeader],
    /// Indexed Design record headers.
    pub(crate) headers: &'a [DesignRecordHeader],
    /// Construction recipes.
    pub(crate) recipes: &'a [ConstructionRecipe],
    /// Byte length of each Design `BulkStream`, by scope.
    pub(crate) stream_lengths: &'a HashMap<String, usize, S>,
}

/// Bind each companion to its exact owned byte interval and the construction
/// recipes nested in that interval. A companion whose payload cannot be
/// resolved is returned unbound.
pub(crate) fn bind_parameter_companion_payloads<S: std::hash::BuildHasher>(
    companions: Vec<DesignParameterCompanion>,
    inputs: &ParameterCompanionInputs<'_, S>,
) -> Result<Vec<DesignParameterCompanion>, CodecError> {
    let mut bound = Vec::new();
    for companion in companions {
        bound.push(match companion_payload(&companion, inputs)? {
            Some(payload) => companion.bound(payload),
            None => companion,
        });
    }
    Ok(bound)
}

/// Resolve the byte interval and nested recipes one companion owns.
fn companion_payload<S: std::hash::BuildHasher>(
    companion: &DesignParameterCompanion,
    inputs: &ParameterCompanionInputs<'_, S>,
) -> Result<Option<crate::records::parameters::DesignCompanionPayload>, CodecError> {
    let ParameterCompanionInputs {
        parameters,
        owners,
        scopes,
        entities,
        headers,
        recipes,
        stream_lengths,
    } = inputs;
    let Some(stream) = native_stream(companion.id()) else {
        return Ok(None);
    };
    let Some(stream_length) = stream_lengths.get(stream).copied() else {
        return Ok(None);
    };
    let Some((start, mut end)) = companion_owned_interval(
        companion,
        parameters.iter(),
        owners,
        scopes,
        headers,
        stream_length,
    ) else {
        return Ok(None);
    };
    // Entity headers precede their owning scope record. A parameter companion
    // immediately before a new scope does not own that scope's preamble even
    // though no indexed sibling separates the two records. Bind the preamble
    // through the scope's entity identity, not by an assumed class-tag or byte
    // length.
    let Ok(preamble_limit) = u64::try_from(end) else {
        return Ok(None);
    };
    end = scopes
        .iter()
        .filter(|scope| {
            native_stream(&scope.id) == Some(stream)
                && scope.byte_offset() >= preamble_limit
                && scope.sketch_entity().is_some()
        })
        .filter_map(|scope| {
            entities
                .iter()
                .filter(|entity| {
                    native_stream(&entity.id) == Some(stream)
                        && scope.sketch_entity().is_some_and(|binding| {
                            binding.entity_id.suffix() == entity.entity_id.suffix()
                        })
                        && entity.byte_offset >= u64_from_index(start)
                        && entity.byte_offset < u64_from_index(end)
                })
                .filter_map(|entity| usize::try_from(entity.byte_offset).ok())
                .min()
        })
        .min()
        .unwrap_or(end);
    let Ok(byte_offset) = u64::try_from(start) else {
        return Ok(None);
    };
    let Ok(byte_length) = u64::try_from(end - start) else {
        return Ok(None);
    };
    let mut owned = Vec::new();
    for recipe in recipes.iter().filter(|recipe| {
        native_stream(&recipe.id) == Some(stream)
            && recipe.byte_offset >= u64_from_index(start)
            && recipe.byte_offset < u64_from_index(end)
    }) {
        owned.push(recipe);
    }
    owned.sort_by(|left, right| {
        let left_key = {
            let recipe = left;
            recipe.byte_offset
        };
        let right_key = {
            let recipe = right;
            recipe.byte_offset
        };
        left_key.cmp(&right_key)
    });
    let mut owned_ids = Vec::new();
    for recipe in owned {
        let id = String::from_utf8(recipe.id.as_bytes().to_vec())
            .map_err(|_| CodecError::malformed("F3D companion recipe ID must be UTF-8"))?;

        owned_ids.push(id);
    }
    Ok(Some(
        crate::records::parameters::DesignCompanionPayload::new(
            byte_offset,
            byte_length,
            owned_ids,
        ),
    ))
}

#[cfg(test)]
mod tests;
