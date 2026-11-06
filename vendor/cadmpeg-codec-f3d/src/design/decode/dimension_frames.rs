// SPDX-License-Identifier: Apache-2.0
//! Parse dimension recipe, locus, and annotation frames.

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::index_from_u32;

use crate::bytes::lp_ascii_filtered_view;
use crate::container::ContainerScan;
use crate::design::construction_recipe_family_name_len;
use crate::design::decode::meta::{decode_types, stream_types_by_class_tag};
use crate::design::decode::sketch::{indexed_record_offsets, next_indexed_record_offset};
use crate::design::decode::text::design_record_id;
use crate::ids::native_stream;
use crate::layout::dimension_annotation_genesis_metadata as annotation_genesis;
use crate::layout::dimension_annotation_metadata_header as annotation_metadata;
use crate::layout::grouped_recipe_reference_prefix as grouped_recipe;
use crate::records::{
    decal::DesignRecordHeader,
    dimensions::{
        DesignDimensionAnnotationFrame, DesignDimensionAnnotationOperand, DesignDimensionLocus,
        DesignDimensionLocusGroup, DesignDimensionLocusPair, DesignDimensionPresentationFrame,
        DesignDimensionPresentationOperand, DesignDimensionRecipeRecord,
    },
    entity_header::DesignEntityHeader,
    feature::scope::DesignParameterScope,
    parameters::{
        DesignParameter, DesignParameterCompanion, DesignParameterKind, DesignParameterOwner,
    },
    recipes::ConstructionRecipe,
    sketch_geometry::{SketchCurveIdentity, SketchPoint},
    sketch_links::PersistentSubentityTag,
    sketch_placement::DesignSketchPlacement,
    topology::edge_identity::DesignEdgeOperand,
};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU32;

/// Record slices every dimension-record decode pass reads: the container scan
/// plus the parameter, owner, companion, scope, record-header, and sketch
/// geometry tables that locate each dimension's owning companion and geometry.
pub(crate) struct DimensionDecodeInputs<'a> {
    pub(crate) scan: &'a ContainerScan<'a>,
    pub(crate) placements: &'a [DesignSketchPlacement],
    pub(crate) parameters: &'a [DesignParameter],
    pub(crate) owners: &'a [DesignParameterOwner],
    pub(crate) companions: &'a [DesignParameterCompanion],
    pub(crate) scopes: &'a [DesignParameterScope],
    pub(crate) headers: &'a [DesignRecordHeader],
    pub(crate) points: &'a [SketchPoint],
    pub(crate) curves: &'a [SketchCurveIdentity],
}

fn dimension_parameter_index(
    parameters: &[DesignParameter],
) -> HashMap<(&str, u32), &DesignParameter> {
    let mut index = HashMap::new();
    for parameter in parameters {
        let Some(stream) = native_stream(&parameter.id) else {
            continue;
        };

        index.insert((stream, parameter.record_index), parameter);
    }
    index
}

fn dimension_companion_keys<'a>(
    owners: &'a [DesignParameterOwner],
    parameters: &HashMap<(&str, u32), &DesignParameter>,
) -> HashSet<(&'a str, u32)> {
    let mut keys = HashSet::new();
    for owner in owners {
        let Some(stream) = native_stream(owner.id()) else {
            continue;
        };
        if parameters
            .get(&(stream, owner.parameter_record_index()))
            .is_some_and(|parameter| parameter.kind() == DesignParameterKind::Dimension)
        {
            keys.insert((stream, owner.companion_record_index()));
        }
    }
    keys
}

fn dimension_geometry_indices(
    stream: &str,
    points: &[SketchPoint],
    curves: &[SketchCurveIdentity],
) -> HashSet<u32> {
    let mut indices = HashSet::new();
    for index in points
        .iter()
        .filter(|point| native_stream(&point.id) == Some(stream))
        .map(|point| point.record_index)
        .chain(
            curves
                .iter()
                .filter(|curve| native_stream(&curve.id) == Some(stream))
                .map(|curve| curve.record_index),
        )
    {
        indices.insert(index);
    }
    indices
}

/// Decode the indexed record that directly contains each construction recipe
/// owned by a dimensional parameter companion.
pub(crate) fn decode_dimension_recipe_records(
    scan: &ContainerScan,
    parameters: &[DesignParameter],
    owners: &[DesignParameterOwner],
    companions: &[DesignParameterCompanion],
    recipes: &[ConstructionRecipe],
) -> Result<Vec<DesignDimensionRecipeRecord>, CodecError> {
    let parameter_index = dimension_parameter_index(parameters);
    let mut dimension_owners = HashSet::new();
    for owner in owners {
        let Some(stream) = native_stream(owner.id()) else {
            continue;
        };
        if parameter_index
            .get(&(stream, owner.parameter_record_index()))
            .is_some_and(|parameter| parameter.kind() == DesignParameterKind::Dimension)
        {
            dimension_owners.insert((stream, owner.record_index()));
        }
    }
    let mut recipe_index = HashMap::new();
    for recipe in recipes {
        recipe_index.insert(recipe.id.as_str(), recipe);
    }
    let mut out = Vec::new();
    for companion in companions.iter().filter(|companion| {
        native_stream(companion.id()).is_some_and(|stream| {
            dimension_owners.contains(&(stream, companion.owner_record_index()))
        })
    }) {
        let Some(stream) = native_stream(companion.id()) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(payload) = companion.payload() else {
            continue;
        };
        let Some(start) = usize::try_from(payload.byte_offset()).ok() else {
            continue;
        };
        let Some(end) = usize::try_from(payload.byte_length())
            .ok()
            .and_then(|length| start.checked_add(length))
            .filter(|end| *end <= bytes.len())
        else {
            continue;
        };
        for (recipe_ordinal, recipe_id) in payload.owned_recipe_ids().iter().enumerate() {
            let Some(recipe) = recipe_index.get(recipe_id.as_str()).copied() else {
                continue;
            };
            let Some(recipe_offset) = usize::try_from(recipe.byte_offset).ok() else {
                continue;
            };
            let Some((at, class_tag, record_index, record_end)) =
                indexed_record_containing(bytes, start, end, recipe_offset)
            else {
                continue;
            };
            let Some(program_offset) = recipe_offset
                .checked_add(construction_recipe_family_name_len(recipe.kind))
                .filter(|offset| *offset < record_end)
            else {
                continue;
            };
            let Some((prefix_offset, prefix_bytes)) = recipe_record_prefix(
                bytes,
                at,
                recipe_offset,
                construction_recipe_family_name_len(recipe.kind),
            ) else {
                continue;
            };
            let Ok(prefix_offset) = u64::try_from(prefix_offset) else {
                continue;
            };
            let references = decode_recipe_references(prefix_bytes, prefix_offset);
            let Some(program) = contiguous_i32_program(bytes, program_offset, record_end) else {
                continue;
            };
            let prefix_bytes = prefix_bytes.to_vec();
            let class_tag = class_tag.to_owned();
            let Ok(class_tag) = crate::records::references::DesignClassTag::try_from(class_tag)
            else {
                continue;
            };
            let (Ok(recipe_ordinal), Ok(byte_offset), Ok(frame_length), Ok(program_offset)) = (
                u32::try_from(recipe_ordinal),
                u64::try_from(at),
                u64::try_from(record_end - at),
                u64::try_from(program_offset),
            ) else {
                continue;
            };
            let id = design_record_id(
                &entry.name,
                ":design-dimension-recipe-record#",
                recipe.byte_offset,
            );
            let recipe_id = recipe.id.clone();

            out.push(DesignDimensionRecipeRecord {
                id,
                companion_record_index: companion.record_index(),
                recipe_ordinal,
                recipe_id,
                recipe_kind: recipe.kind,
                byte_offset,
                class_tag,
                record_index,
                frame_length,
                prefix_offset,
                prefix_bytes: prefix_bytes.into(),
                references,
                program_offset,
                program,
                matching_edge_operand_ids: Vec::new(),
            });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(crate) fn decode_recipe_references(
    prefix: &[u8],
    prefix_offset: u64,
) -> Vec<crate::records::dimensions::DesignRecipeReference> {
    if prefix
        .get(..10)
        .is_none_or(|bytes| bytes.iter().any(|byte| *byte != 0))
        || View::u32_le_at(prefix, 10) != Some(1)
        || View::u32_le_at(prefix, 18).is_none_or(|value| value == 0)
    {
        return Vec::new();
    }
    match View::u32_le_at(prefix, 14) {
        Some(2) => decode_paired_recipe_references(prefix, prefix_offset),
        Some(3) => decode_standard_recipe_references(prefix, prefix_offset),
        Some(group_count) if group_count >= 4 => usize::try_from(group_count)
            .ok()
            .map_or_else(Vec::new, |group_count| {
                decode_grouped_recipe_references(prefix, prefix_offset, group_count)
            }),
        _ => Vec::new(),
    }
}

fn decode_standard_recipe_references(
    prefix: &[u8],
    prefix_offset: u64,
) -> Vec<crate::records::dimensions::DesignRecipeReference> {
    if View::u32_le_at(prefix, 22).is_none_or(|value| value == 0) {
        return Vec::new();
    }
    let mut references = Vec::new();
    let mut at = 22usize;
    while prefix
        .len()
        .checked_sub(at)
        .is_some_and(|remaining| remaining > 4)
    {
        if recipe_reference_suffix(&prefix[at..]) {
            return references;
        }
        let Some(parsed) = decode_recipe_reference_operand(
            prefix,
            prefix_offset,
            at,
            RecipeReferenceTokenFrame::Either,
        ) else {
            return Vec::new();
        };
        let DecodedRecipeReferenceOperand {
            references: operand_references,
            next,
        } = parsed;
        references.extend(operand_references);
        at = next;
    }
    if prefix.get(at..) == Some(&[0, 0, 0, 0]) {
        references
    } else {
        Vec::new()
    }
}

fn decode_paired_recipe_references(
    prefix: &[u8],
    prefix_offset: u64,
) -> Vec<crate::records::dimensions::DesignRecipeReference> {
    const MINIMUM_PAIR_SIZE: usize = 42;

    let Some(pair_count) = View::u32_le_at(prefix, 18).map(index_from_u32) else {
        return Vec::new();
    };
    if pair_count == 0
        || prefix
            .len()
            .checked_sub(22)
            .is_none_or(|remaining| pair_count > remaining / MINIMUM_PAIR_SIZE)
    {
        return Vec::new();
    }
    if pair_count.checked_mul(2).is_none() {
        return Vec::new();
    }
    let mut at = 22usize;
    let mut operands = Vec::new();
    for _ in 0..pair_count {
        let Some(parsed) = decode_recipe_reference_operand(
            prefix,
            prefix_offset,
            at,
            RecipeReferenceTokenFrame::Packed,
        ) else {
            return Vec::new();
        };
        let DecodedRecipeReferenceOperand {
            references: packed,
            next,
        } = parsed;
        at = next;
        let Some(parsed) = decode_recipe_reference_operand(
            prefix,
            prefix_offset,
            at,
            RecipeReferenceTokenFrame::LengthPrefixed,
        ) else {
            return Vec::new();
        };
        let DecodedRecipeReferenceOperand {
            references: length_prefixed,
            next,
        } = parsed;
        operands.push(packed);
        operands.push(length_prefixed);
        at = next;
    }
    if at != prefix.len()
        || operands.chunks_exact(2).any(|pair| {
            pair[0].first().map(|reference| reference.selector)
                != pair[1].first().map(|reference| reference.selector)
                || pair[0]
                    .iter()
                    .map(|reference| reference.design_reference)
                    .ne(pair[1].iter().map(|reference| reference.design_reference))
        })
    {
        return Vec::new();
    }
    operands.into_iter().flatten().collect()
}

fn decode_grouped_recipe_references(
    prefix: &[u8],
    prefix_offset: u64,
    group_count: usize,
) -> Vec<crate::records::dimensions::DesignRecipeReference> {
    const MINIMUM_PACKED_OPERAND_SIZE: usize = 17;
    const GROUP_COUNT_WORD_SIZE: usize = 4;

    let mut references = Vec::new();
    let mut at = grouped_recipe::LEN;
    // `group_count` is parsed, so both refusals below are reachable: a prefix
    // shorter than the grouped-recipe header states no group at all, and a
    // group count whose packed groups do not fit the prefix states no group
    // this decoder can read. The multiplication states the same bound the
    // division stated, without a divisor whose zero case no input reaches.
    let Some(available) = prefix.len().checked_sub(at) else {
        return Vec::new();
    };
    let Some(required) =
        group_count.checked_mul(GROUP_COUNT_WORD_SIZE + MINIMUM_PACKED_OPERAND_SIZE)
    else {
        return Vec::new();
    };
    if required > available {
        return Vec::new();
    }
    for _ in 0..group_count {
        let Some(operand_count) = View::u32_le_at(prefix, at).map(index_from_u32) else {
            return Vec::new();
        };
        let Some(next) = at.checked_add(4) else {
            return Vec::new();
        };
        at = next;
        if operand_count == 0
            || prefix
                .len()
                .checked_sub(at)
                .is_none_or(|remaining| operand_count > remaining / MINIMUM_PACKED_OPERAND_SIZE)
        {
            return Vec::new();
        }
        for _ in 0..operand_count {
            let Some(parsed) = decode_recipe_reference_operand(
                prefix,
                prefix_offset,
                at,
                RecipeReferenceTokenFrame::Packed,
            ) else {
                return Vec::new();
            };
            let DecodedRecipeReferenceOperand {
                references: operand_references,
                next,
            } = parsed;
            references.extend(operand_references);
            at = next;
        }
    }
    if prefix.get(at..) == Some(&[0, 0, 0, 0]) {
        references
    } else {
        Vec::new()
    }
}

pub(in crate::design) fn is_paired_recipe_reference_frame(prefix: &[u8]) -> bool {
    if !recipe_reference_header(prefix)
        || View::u32_le_at(prefix, grouped_recipe::GROUP_COUNT) != Some(2)
    {
        return false;
    }
    let Some(pair_count) = View::u32_le_at(prefix, 18).map(index_from_u32) else {
        return false;
    };
    if pair_count == 0
        || prefix
            .len()
            .checked_sub(22)
            .is_none_or(|remaining| pair_count > remaining / 42)
    {
        return false;
    }
    let mut at = 22usize;
    for _ in 0..pair_count {
        let Some(packed) =
            scan_recipe_reference_operand(prefix, at, RecipeReferenceTokenFrame::Packed)
        else {
            return false;
        };
        let Some(length_prefixed) = scan_recipe_reference_operand(
            prefix,
            packed.next,
            RecipeReferenceTokenFrame::LengthPrefixed,
        ) else {
            return false;
        };
        if packed.selector != length_prefixed.selector
            || packed.reference_count != length_prefixed.reference_count
            || (0..packed.reference_count).any(|ordinal| {
                let offset = ordinal * 4;
                View::u32_le_at(prefix, packed.references_at + offset)
                    != View::u32_le_at(prefix, length_prefixed.references_at + offset)
            })
        {
            return false;
        }
        at = length_prefixed.next;
    }
    at == prefix.len()
}

pub(crate) fn is_grouped_recipe_reference_frame(prefix: &[u8]) -> bool {
    if !recipe_reference_header(prefix) {
        return false;
    }
    let Some(group_count) = View::u32_le_at(prefix, grouped_recipe::GROUP_COUNT)
        .filter(|count| *count >= 4)
        .map(index_from_u32)
    else {
        return false;
    };
    let Some(available) = prefix.len().checked_sub(grouped_recipe::LEN) else {
        return false;
    };
    let Some(required) = group_count.checked_mul(21) else {
        return false;
    };
    if required > available {
        return false;
    }
    let mut at = grouped_recipe::LEN;
    for _ in 0..group_count {
        let Some(operand_count) = View::u32_le_at(prefix, at).map(index_from_u32) else {
            return false;
        };
        let Some(next) = at.checked_add(4) else {
            return false;
        };
        at = next;
        if operand_count == 0
            || prefix
                .len()
                .checked_sub(at)
                .is_none_or(|remaining| operand_count > remaining / 17)
        {
            return false;
        }
        for _ in 0..operand_count {
            let Some(operand) =
                scan_recipe_reference_operand(prefix, at, RecipeReferenceTokenFrame::Packed)
            else {
                return false;
            };
            at = operand.next;
        }
    }
    prefix.get(at..) == Some(&[0, 0, 0, 0])
}

fn recipe_reference_header(prefix: &[u8]) -> bool {
    prefix
        .get(..10)
        .is_some_and(|bytes| bytes.iter().all(|byte| *byte == 0))
        && View::u32_le_at(prefix, 10) == Some(1)
        && View::u32_le_at(prefix, 18).is_some_and(|value| value != 0)
}

#[derive(Clone, Copy)]
enum RecipeReferenceTokenFrame {
    Either,
    Packed,
    LengthPrefixed,
}

struct ScannedRecipeReferenceOperand<'a> {
    selector: u32,
    token: &'a str,
    token_at: usize,
    references_at: usize,
    reference_count: usize,
    next: usize,
}

fn scan_recipe_reference_operand(
    prefix: &[u8],
    at: usize,
    token_frame: RecipeReferenceTokenFrame,
) -> Option<ScannedRecipeReferenceOperand<'_>> {
    let selector = View::u32_le_at(prefix, at).filter(|value| *value != 0)?;
    let token_encoding_at = at.checked_add(4)?;
    let length_prefixed = (!matches!(token_frame, RecipeReferenceTokenFrame::Packed))
        .then(|| {
            lp_ascii_filtered_view(prefix, token_encoding_at, 0..=2000, u8::is_ascii_graphic)
                .and_then(|(token, marker_at)| {
                    (is_decimal_integer_token(token.as_bytes())
                        && View::u32_le_at(prefix, marker_at) == Some(0))
                    .then_some((token, token_encoding_at + 4, marker_at + 4))
                })
        })
        .flatten();
    let packed = (!matches!(token_frame, RecipeReferenceTokenFrame::LengthPrefixed))
        .then(|| {
            (1usize..=8).find_map(|length| {
                let token =
                    prefix.get(token_encoding_at..token_encoding_at.checked_add(length)?)?;
                let zero_at = token_encoding_at.checked_add(length)?;
                (is_decimal_integer_token(token)
                    && prefix.get(zero_at..zero_at + 4) == Some(&[0; 4]))
                .then(|| std::str::from_utf8(token).ok())
                .flatten()
                .map(|token| (token, token_encoding_at, zero_at + 4))
            })
        })
        .flatten();
    let (token, token_at, marker_at) = length_prefixed.or(packed)?;
    let reference_count =
        usize::try_from(View::u32_le_at(prefix, marker_at).filter(|value| *value != 0)?).ok()?;
    let reference_bytes = reference_count.checked_mul(4)?;
    let references_at = marker_at.checked_add(4)?;
    if reference_bytes > prefix.len().checked_sub(references_at)? {
        return None;
    }
    let references_end = references_at.checked_add(reference_bytes)?;
    let next = match token_frame {
        RecipeReferenceTokenFrame::Packed => references_end,
        RecipeReferenceTokenFrame::Either | RecipeReferenceTokenFrame::LengthPrefixed => {
            if View::u32_le_at(prefix, references_end) != Some(0) {
                return None;
            }
            references_end.checked_add(4)?
        }
    };
    for reference_ordinal in 0..reference_count {
        let design_reference_at = references_at.checked_add(reference_ordinal.checked_mul(4)?)?;
        View::i32_le_at(prefix, design_reference_at).filter(|value| *value != 0)?;
    }
    Some(ScannedRecipeReferenceOperand {
        selector,
        token,
        token_at,
        references_at,
        reference_count,
        next,
    })
}

struct DecodedRecipeReferenceOperand {
    references: Vec<crate::records::dimensions::DesignRecipeReference>,
    next: usize,
}

fn decode_recipe_reference_operand(
    prefix: &[u8],
    prefix_offset: u64,
    at: usize,
    token_frame: RecipeReferenceTokenFrame,
) -> Option<DecodedRecipeReferenceOperand> {
    let scanned = scan_recipe_reference_operand(prefix, at, token_frame)?;
    let selector_offset = prefix_offset.checked_add(u64::try_from(at).ok()?)?;
    let token_offset = prefix_offset.checked_add(u64::try_from(scanned.token_at).ok()?)?;
    let mut references = Vec::new();
    for reference_ordinal in 0..scanned.reference_count {
        let design_reference_at = scanned
            .references_at
            .checked_add(reference_ordinal.checked_mul(4)?)?;
        let design_reference =
            View::i32_le_at(prefix, design_reference_at).filter(|value| *value != 0)?;
        let design_reference_offset =
            prefix_offset.checked_add(u64::try_from(design_reference_at).ok()?)?;
        let token_copy = scanned.token.to_owned();
        let reference = crate::records::dimensions::DesignRecipeReference {
            selector: i64::from(scanned.selector),
            selector_offset,
            token: token_copy,
            token_offset,
            design_reference: i64::from(design_reference),
            design_reference_offset,
            candidate_faces: Vec::new(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
        };
        references.push(reference);
    }
    Some(DecodedRecipeReferenceOperand {
        references,
        next: scanned.next,
    })
}

fn is_decimal_integer_token(token: &[u8]) -> bool {
    let digits = token.strip_prefix(b"-").unwrap_or(token);
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}

fn recipe_reference_suffix(bytes: &[u8]) -> bool {
    if bytes == [0; 4] {
        return true;
    }
    if View::u32_le_at(bytes, 0) != Some(1)
        || View::u32_le_at(bytes, 4) != Some(1)
        || View::u32_le_at(bytes, 8) != Some(0)
        || View::u32_le_at(bytes, 12) != Some(0)
    {
        return false;
    }
    let Some(reference_count) = View::u32_le_at(bytes, 16).filter(|count| *count != 0) else {
        return false;
    };
    let Some(reference_bytes) = usize::try_from(reference_count)
        .ok()
        .and_then(|count| count.checked_mul(4))
    else {
        return false;
    };
    let Some(terminator_at) = 20usize.checked_add(reference_bytes) else {
        return false;
    };
    matches!(bytes.len().checked_sub(terminator_at), Some(4 | 6))
        && (0..index_from_u32(reference_count)).all(|ordinal| {
            View::u32_le_at(bytes, 20 + 4 * ordinal).is_some_and(|reference| reference != 0)
        })
        && bytes[terminator_at..].iter().all(|byte| *byte == 0)
}

/// Join dimension-recipe selector/reference pairs to active solved subentities.
pub(crate) fn bind_dimension_recipe_reference_candidates(
    records: &mut [DesignDimensionRecipeRecord],
    tags: &[PersistentSubentityTag],
) {
    for record in records {
        for reference in &mut record.references {
            bind_recipe_reference_candidates(reference, tags, Some(&record.id));
        }
    }
}

pub(crate) fn bind_recipe_reference_candidates(
    reference: &mut crate::records::dimensions::DesignRecipeReference,
    tags: &[PersistentSubentityTag],
    owner_id: Option<&str>,
) {
    use cadmpeg_ir::attributes::AttributeTarget;

    reference.candidate_faces.clear();
    reference.candidate_edges.clear();
    reference.alternate_selector_faces.clear();
    reference.alternate_selector_edges.clear();
    for tag in tags.iter().filter(|tag| {
        tag.token.as_str() == reference.token
            && tag.design_references.contains(&reference.design_reference)
            && owner_id.is_none_or(|owner_id| crate::ids::same_native_occurrence(&tag.id, owner_id))
    }) {
        let matching_selector = tag.selector == reference.selector;
        match (&tag.target, matching_selector) {
            (AttributeTarget::Face(face), true) => reference.candidate_faces.push(face.clone()),
            (AttributeTarget::Edge(edge), true) => reference.candidate_edges.push(edge.clone()),
            (AttributeTarget::Face(face), false) => {
                reference.alternate_selector_faces.push(face.clone());
            }
            (AttributeTarget::Edge(edge), false) => {
                reference.alternate_selector_edges.push(edge.clone());
            }
            _ => {}
        }
    }
    reference
        .candidate_faces
        .sort_by(|a, b| a.as_str().cmp(b.as_str()));
    reference.candidate_faces.dedup();
    reference
        .candidate_edges
        .sort_by(|a, b| a.as_str().cmp(b.as_str()));
    reference.candidate_edges.dedup();
    reference
        .alternate_selector_faces
        .sort_by(|a, b| a.as_str().cmp(b.as_str()));
    reference.alternate_selector_faces.dedup();
    reference
        .alternate_selector_edges
        .sort_by(|a, b| a.as_str().cmp(b.as_str()));
    reference.alternate_selector_edges.dedup();
}

/// Join dimension programs to byte-identical edge-recipe program tails.
pub(crate) fn bind_dimension_recipe_edge_operands(
    records: &mut [DesignDimensionRecipeRecord],
    operands: &[DesignEdgeOperand],
) {
    for record in records {
        let ids = dimension_recipe_matching_edge_operand_ids(record, operands);
        record.matching_edge_operand_ids = ids;
    }
}

pub(crate) fn dimension_recipe_matching_edge_operand_ids(
    record: &DesignDimensionRecipeRecord,
    operands: &[DesignEdgeOperand],
) -> Vec<String> {
    let mut ids: Vec<_> = operands
        .iter()
        .filter(|operand| dimension_recipe_edge_matches(record, operand))
        .map(|operand| operand.id.clone())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn dimension_recipe_edge_matches(
    record: &DesignDimensionRecipeRecord,
    operand: &DesignEdgeOperand,
) -> bool {
    if native_stream(&operand.id) != native_stream(&record.id) {
        return false;
    }
    let Some(prelude_length) = super::operands::edge_recipe_prelude_length(
        &operand.recipe_program,
        &cadmpeg_core::decode::WorkBudget::new(super::operands::RECIPE_SIDE_SEARCH_WORK),
    ) else {
        return false;
    };
    let Some(tail) = operand
        .recipe_program
        .get(prelude_length..)
        .filter(|tail| !tail.is_empty())
    else {
        return false;
    };
    record
        .program
        .windows(tail.len())
        .any(|window| window == tail)
}

pub(super) fn recipe_record_prefix(
    bytes: &[u8],
    record_offset: usize,
    family_name_offset: usize,
    family_name_len: usize,
) -> Option<(usize, &[u8])> {
    let prefix_offset = record_offset.checked_add(11)?;
    let prefix_end = family_name_offset.checked_sub(4)?;
    if View::u32_le_at(bytes, prefix_end)? != u32::try_from(family_name_len).ok()? {
        return None;
    }
    let prefix = bytes.get(prefix_offset..prefix_end)?;
    Some((prefix_offset, prefix))
}

fn indexed_record_containing(
    bytes: &[u8],
    start: usize,
    end: usize,
    member_offset: usize,
) -> Option<(usize, &str, u32, usize)> {
    if start > member_offset || member_offset >= end || end > bytes.len() {
        return None;
    }
    let mut cursor = start;
    let mut containing = None;
    while let Some(at) = next_indexed_record_offset(bytes, cursor) {
        if at >= end {
            break;
        }
        if at > member_offset {
            return containing
                .map(|(offset, class_tag, record_index)| (offset, class_tag, record_index, at));
        }
        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
        containing = Some((at, class_tag, View::u32_le_at(bytes, after_tag)?));
        cursor = at + 11;
    }
    containing.map(|(offset, class_tag, record_index)| (offset, class_tag, record_index, end))
}

pub(super) fn contiguous_i32_program(bytes: &[u8], start: usize, end: usize) -> Option<Vec<i32>> {
    let view = View::over_retained(bytes).child(start, end)?;
    if view.remaining() == 0 || !view.remaining().is_multiple_of(4) {
        return None;
    }

    let mut program = Vec::new();
    for at in (start..end).step_by(4) {
        program.push(View::i32_le_at(bytes, at)?);
    }
    Some(program)
}

/// Decode paired typed sketch loci nested immediately after dimensional
/// parameter-companion prefixes.
pub(crate) fn decode_dimension_locus_pairs(
    inputs: &DimensionDecodeInputs<'_>,
) -> Result<Vec<DesignDimensionLocusPair>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        scopes,
        headers,
        points,
        curves,
        ..
    } = inputs;
    let parameters = dimension_parameter_index(parameters);
    let dimension_companions = dimension_companion_keys(owners, &parameters);
    let mut out = Vec::new();
    for (companion, scope) in companions.iter().filter_map(|companion| {
        let scope = native_stream(companion.id())?;
        dimension_companions
            .contains(&(scope, companion.record_index()))
            .then_some((companion, scope))
    }) {
        let entry = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, scope);
        let Some(entry) = entry else {
            continue;
        };
        let geometry_indices = dimension_geometry_indices(scope, points, curves);
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.values().copied(),
            owners,
            scopes,
            headers,
            bytes.len(),
        ) else {
            continue;
        };
        let Some(mut pair) = find_dimension_locus_pair(
            bytes,
            start,
            end,
            companion.record_index(),
            &geometry_indices,
        ) else {
            continue;
        };
        pair.id = design_record_id(
            &entry.name,
            ":design-dimension-locus-pair#",
            pair.byte_offset(),
        );
        let Some(governing_companion_record_index) = following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset(),
            owners,
            parameters.values().copied(),
        ) else {
            continue;
        };
        pair.governing_companion_record_index = governing_companion_record_index;

        out.push(pair);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(crate) fn following_dimension_companion_record_index<'a, I>(
    native_id: &str,
    paired_byte_offset: u64,
    owners: &[DesignParameterOwner],
    parameter_records: I,
) -> Option<u32>
where
    I: IntoIterator<Item = &'a DesignParameter>,
    I::IntoIter: Clone,
{
    let scope = native_stream(native_id)?;
    let parameter_records = parameter_records.into_iter();
    let following_offset = paired_byte_offset.checked_add(59)?;
    let mut matches = owners.iter().filter(|owner| {
        native_stream(owner.id()) == Some(scope) && owner.byte_offset() == following_offset && {
            let mut candidates = parameter_records.clone().filter(|parameter| {
                native_stream(&parameter.id) == Some(scope)
                    && parameter.record_index == owner.parameter_record_index()
            });
            candidates.next().is_some_and(|parameter| {
                parameter.kind() == DesignParameterKind::Dimension && candidates.next().is_none()
            })
        }
    });
    let owner = matches.next()?;
    matches
        .next()
        .is_none()
        .then_some(owner.companion_record_index())
}

fn find_dimension_locus_pair(
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionLocusPair> {
    let parse = |at| {
        parse_dimension_locus_pair(bytes, at, companion_record_index, geometry_indices)
            .filter(|pair| pair.paired_byte_offset() < u64_from_index(end))
    };
    let mut candidate = parse(start);
    let mut position = start.checked_add(1);
    while let Some(at) = position.and_then(|position| next_indexed_record_offset(bytes, position)) {
        if at >= end {
            break;
        }
        if let Some(pair) = parse(at) {
            if candidate.is_some() {
                return None;
            }
            candidate = Some(pair);
        }
        position = at.checked_add(1);
    }
    candidate
}

fn parse_dimension_locus_pair(
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionLocusPair> {
    let (class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    let record_index = View::u32_le_at(bytes, after_tag)?;
    if after_tag != start.checked_add(7)?
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
        || View::u32_le_at(bytes, start + 20) != Some(3)
        || bytes.get(start + 24) != Some(&1)
        || View::u32_le_at(bytes, start + 25) != Some(0)
        || bytes.get(start + 29..start + 35) != Some(&[0; 6])
        || bytes.get(start + 39) != Some(&1)
        || bytes.get(start + 44..start + 50) != Some(&[0; 6])
        || bytes.get(start + 54) != Some(&1)
        || bytes.get(start + 59..start + 65) != Some(&[0; 6])
    {
        return None;
    }
    let first_geometry_record_index = View::u32_le_at(bytes, start + 40)?;
    let second_geometry_record_index = View::u32_le_at(bytes, start + 55)?;
    if !geometry_indices.contains(&first_geometry_record_index)
        || !geometry_indices.contains(&second_geometry_record_index)
    {
        return None;
    }
    let mut position = start.checked_add(69)?;
    let (paired_byte_offset, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, position)?;
        let (candidate_tag, candidate_after_tag) =
            lp_ascii_filtered_view(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, candidate_after_tag) == Some(record_index) {
            break (at, candidate_tag);
        }
        position = at.checked_add(1)?;
    };
    DesignDimensionLocusPair::try_new(crate::records::dimensions::DesignDimensionLocusPairDraft {
        id: String::new(),
        companion_record_index,
        governing_companion_record_index: companion_record_index,
        byte_offset: u64_from_index(start),
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        record_index,
        frame_length: u64::try_from(paired_byte_offset.checked_sub(start)?).ok()?,
        opaque_index: Some(crate::records::identity::Located {
            value: View::u32_le_at(bytes, start + 35)?,
            offset: u64_from_index(start + 35),
        }),
        loci: [
            crate::records::dimensions::DesignDimensionAnnotationOperand {
                geometry_record_index: Some(NonZeroU32::new(first_geometry_record_index)?),
                geometry_reference_offset: u64_from_index(start + 40),
                role: View::u32_le_at(bytes, start + 50)?,
                role_offset: u64_from_index(start + 50),
            },
            crate::records::dimensions::DesignDimensionAnnotationOperand {
                geometry_record_index: Some(NonZeroU32::new(second_geometry_record_index)?),
                geometry_reference_offset: u64_from_index(start + 55),
                role: View::u32_le_at(bytes, start + 65)?,
                role_offset: u64_from_index(start + 65),
            },
        ],
        paired_class_tag: crate::design::decode::text::class_tag_from_view(paired_class_tag)
            .ok()?,
        paired_byte_offset: u64_from_index(paired_byte_offset),
    })
    .ok()
}

/// Decode dimension frames whose ordered operand run contains a null record
/// reference followed by one typed sketch-geometry reference.
pub(crate) fn decode_dimension_null_locus_pairs(
    inputs: &DimensionDecodeInputs<'_>,
    pairs: &[DesignDimensionLocusPair],
    groups: &[DesignDimensionLocusGroup],
) -> Result<Vec<DesignDimensionLocusPair>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        scopes,
        headers,
        points,
        curves,
        ..
    } = inputs;
    let parameters = dimension_parameter_index(parameters);
    let dimension_companions = dimension_companion_keys(owners, &parameters);
    let mut typed_companions = HashSet::new();
    for key in
        pairs
            .iter()
            .filter_map(|pair| Some((native_stream(&pair.id)?, pair.companion_record_index)))
            .chain(groups.iter().filter_map(|group| {
                Some((native_stream(&group.id)?, group.companion_record_index))
            }))
    {
        typed_companions.insert(key);
    }
    let mut out = Vec::new();
    for (companion, scope) in companions.iter().filter_map(|companion| {
        let scope = native_stream(companion.id())?;
        let key = (scope, companion.record_index());
        (dimension_companions.contains(&key) && !typed_companions.contains(&key))
            .then_some((companion, scope))
    }) {
        let entry = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, scope);
        let Some(entry) = entry else {
            continue;
        };
        let geometry_indices = dimension_geometry_indices(scope, points, curves);
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.values().copied(),
            owners,
            scopes,
            headers,
            bytes.len(),
        ) else {
            continue;
        };
        let Some(mut pair) = find_dimension_null_locus_pair(
            bytes,
            start,
            end,
            companion.record_index(),
            &geometry_indices,
        ) else {
            continue;
        };
        pair.id = design_record_id(
            &entry.name,
            ":design-dimension-null-locus-pair#",
            pair.byte_offset(),
        );
        let Some(governing_companion_record_index) = following_dimension_companion_record_index(
            &pair.id,
            pair.paired_byte_offset(),
            owners,
            parameters.values().copied(),
        ) else {
            continue;
        };
        pair.governing_companion_record_index = governing_companion_record_index;

        out.push(pair);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn find_dimension_null_locus_pair(
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionLocusPair> {
    let parse = |at| {
        parse_dimension_null_locus_pair(bytes, at, companion_record_index, geometry_indices)
            .filter(|pair| pair.paired_byte_offset() < u64_from_index(end))
    };
    let mut candidate = parse(start);
    let mut position = start.checked_add(1);
    while let Some(at) = position.and_then(|position| next_indexed_record_offset(bytes, position)) {
        if at >= end {
            break;
        }
        if let Some(pair) = parse(at) {
            if candidate.is_some() {
                return None;
            }
            candidate = Some(pair);
        }
        position = at.checked_add(1);
    }
    candidate
}

fn parse_dimension_null_locus_pair(
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
) -> Option<DesignDimensionLocusPair> {
    let (class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    let record_index = View::u32_le_at(bytes, after_tag)?;
    if after_tag != start.checked_add(7)?
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
        || View::u32_le_at(bytes, start + 20) != Some(2)
        || bytes.get(start + 24) != Some(&1)
        || View::u32_le_at(bytes, start + 25) != Some(0)
        || bytes.get(start + 29..start + 35) != Some(&[0; 6])
        || bytes.get(start + 39) != Some(&1)
        || bytes.get(start + 44..start + 50) != Some(&[0; 6])
    {
        return None;
    }
    let geometry_record_index = View::u32_le_at(bytes, start + 40)?;
    if !geometry_indices.contains(&geometry_record_index) {
        return None;
    }
    let mut position = start.checked_add(54)?;
    let (paired_byte_offset, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, position)?;
        let (candidate_tag, candidate_after_tag) =
            lp_ascii_filtered_view(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, candidate_after_tag) == Some(record_index) {
            break (at, candidate_tag);
        }
        position = at.checked_add(1)?;
    };
    DesignDimensionLocusPair::try_new(crate::records::dimensions::DesignDimensionLocusPairDraft {
        id: String::new(),
        companion_record_index,
        governing_companion_record_index: companion_record_index,
        byte_offset: u64_from_index(start),
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        record_index,
        frame_length: u64::try_from(paired_byte_offset.checked_sub(start)?).ok()?,
        opaque_index: None,
        loci: [
            crate::records::dimensions::DesignDimensionAnnotationOperand {
                geometry_record_index: None,
                geometry_reference_offset: u64_from_index(start + 25),
                role: View::u32_le_at(bytes, start + 35)?,
                role_offset: u64_from_index(start + 35),
            },
            crate::records::dimensions::DesignDimensionAnnotationOperand {
                geometry_record_index: Some(NonZeroU32::new(geometry_record_index)?),
                geometry_reference_offset: u64_from_index(start + 40),
                role: View::u32_le_at(bytes, start + 50)?,
                role_offset: u64_from_index(start + 50),
            },
        ],
        paired_class_tag: crate::design::decode::text::class_tag_from_view(paired_class_tag)
            .ok()?,
        paired_byte_offset: u64_from_index(paired_byte_offset),
    })
    .ok()
}

/// Decode paired `EntityGenesis` dimensional frames carrying annotation data
/// and a direct backlink to the governed parameter owner.
pub(crate) fn decode_dimension_annotation_frames(
    inputs: &DimensionDecodeInputs<'_>,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignDimensionAnnotationFrame>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        scopes,
        headers,
        points,
        curves,
        ..
    } = inputs;
    let parameters = dimension_parameter_index(parameters);
    let dimension_companions = dimension_companion_keys(owners, &parameters);
    let mut out = Vec::new();
    let mut decoded_offsets = HashSet::new();
    for (companion_ordinal, companion) in companions.iter().enumerate() {
        let Some(stream) = native_stream(companion.id()) else {
            continue;
        };
        if companions[..companion_ordinal]
            .iter()
            .any(|previous| native_stream(previous.id()) == Some(stream))
        {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let geometry_indices = dimension_geometry_indices(stream, points, curves);
        let mut sketch_entities = HashSet::new();
        for entity in entities
            .iter()
            .filter(|entity| native_stream(&entity.id) == Some(stream) && entity.in_sketch_module())
        {
            let Ok(index) = u32::try_from(entity.entity_id.suffix()) else {
                continue;
            };

            sketch_entities.insert(index);
        }
        let mut governed_owners = HashMap::new();
        for owner in owners.iter().filter(|owner| {
            native_stream(owner.id()) == Some(stream)
                && dimension_companions.contains(&(stream, owner.companion_record_index()))
        }) {
            governed_owners.insert(owner.record_index(), owner.companion_record_index());
        }
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut intervals = Vec::new();
        for companion in companions
            .iter()
            .filter(|companion| native_stream(companion.id()) == Some(stream))
        {
            let Some((start, end)) = companion_owned_interval(
                companion,
                parameters.values().copied(),
                owners,
                scopes,
                headers,
                bytes.len(),
            ) else {
                continue;
            };

            intervals.push((start, end, Some(companion.record_index())));
        }
        for interval in scopes.iter().filter_map(|scope| {
            if native_stream(&scope.id) != Some(stream) {
                return None;
            }
            let end = owners
                .iter()
                .filter(|owner| {
                    native_stream(owner.id()) == Some(stream)
                        && owner.scope_record_index() == scope.record_index
                })
                .filter_map(|owner| {
                    companions
                        .iter()
                        .find(|companion| {
                            native_stream(companion.id()) == Some(stream)
                                && companion.record_index() == owner.companion_record_index()
                        })
                        .and_then(|companion| usize::try_from(companion.byte_offset()).ok())
                })
                .min()?;
            let start = usize::try_from(scope.byte_offset()).ok()?;
            (start < end).then_some((start, end, None))
        }) {
            intervals.push(interval);
        }
        for (start, end, containing_companion_record_index) in intervals {
            let mut position = start;
            while position < end {
                let at = next_indexed_record_offset(bytes, position);
                let Some(at) = at.filter(|at| *at < end) else {
                    break;
                };
                if let Some(mut frame) = parse_dimension_annotation_frame(
                    bytes,
                    at,
                    containing_companion_record_index,
                    &governed_owners,
                    &geometry_indices,
                    &sketch_entities,
                ) {
                    if frame.paired_byte_offset() >= u64_from_index(end) {
                        position = at + 1;
                        continue;
                    }
                    frame.id = design_record_id(
                        &entry.name,
                        ":design-dimension-annotation-frame#",
                        frame.byte_offset(),
                    );
                    let Ok(paired_at) = usize::try_from(frame.paired_byte_offset()) else {
                        return Err(CodecError::Malformed(
                            "F3D annotation paired offset is not representable".into(),
                        ));
                    };
                    position = paired_at + 1;
                    let key = (stream, frame.byte_offset());
                    if !decoded_offsets.contains(&key) {
                        decoded_offsets.insert(key);

                        out.push(frame);
                    }
                } else {
                    position = at + 1;
                }
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn parse_dimension_annotation_frame(
    bytes: &[u8],
    start: usize,
    companion_record_index: Option<u32>,
    governed_owners: &HashMap<u32, u32>,
    geometry_indices: &HashSet<u32>,
    sketch_entities: &HashSet<u32>,
) -> Option<DesignDimensionAnnotationFrame> {
    let (class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    if after_tag != start.checked_add(7)?
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
    {
        return None;
    }
    let record_index = View::u32_le_at(bytes, after_tag)?;
    let count = usize::try_from(View::u32_le_at(bytes, start + 20)?).ok()?;
    if !(1..=64).contains(&count) {
        return None;
    }
    let mut position = start.checked_add(24)?;

    let mut operands = Vec::new();
    for _ in 0..count {
        if bytes.get(position) != Some(&1)
            || bytes.get(position + 5..position + 11) != Some(&[0; 6])
        {
            return None;
        }
        let geometry_record_index =
            std::num::NonZeroU32::new(View::u32_le_at(bytes, position + 1)?);
        if geometry_record_index.is_some_and(|index| !geometry_indices.contains(&index.get())) {
            return None;
        }
        operands.push(DesignDimensionAnnotationOperand {
            geometry_record_index,
            geometry_reference_offset: u64_from_index(position + 1),
            role: View::u32_le_at(bytes, position + 11)?,
            role_offset: u64_from_index(position + 11),
        });
        position = position.checked_add(15)?;
    }
    if bytes.get(position + annotation_metadata::PRESENT)
        != Some(&annotation_metadata::PRESENT_VALUE)
    {
        return None;
    }
    let (entity_genesis, annotation_byte_offset) =
        match View::u32_le_at(bytes, position + annotation_metadata::ENTRY_COUNT)? {
            0 => (None, position.checked_add(annotation_metadata::LEN)?),
            annotation_genesis::ENTRY_COUNT_VALUE => {
                let (key, after_key) = lp_ascii_filtered_view(
                    bytes,
                    position.checked_add(annotation_metadata::LEN)?,
                    0..=2000,
                    u8::is_ascii_graphic,
                )?;
                let (meta_type, after_type) =
                    lp_ascii_filtered_view(bytes, after_key, 0..=2000, u8::is_ascii_graphic)?;
                if key.as_bytes() != annotation_genesis::KEY_VALUE.as_slice()
                    || meta_type.as_bytes() != annotation_genesis::META_TYPE_VALUE.as_slice()
                    || after_type != position.checked_add(annotation_genesis::ENTITY_GENESIS)?
                {
                    return None;
                }
                (
                    Some(View::u64_le_at(bytes, after_type)?),
                    position.checked_add(annotation_genesis::LEN)?,
                )
            }
            _ => return None,
        };
    let mut paired_search = annotation_byte_offset;
    let (paired_byte_offset, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, paired_search)?;
        let (tag, after) = lp_ascii_filtered_view(bytes, at, 0..=2000, u8::is_ascii_graphic)?;
        if View::u32_le_at(bytes, after) == Some(record_index) {
            break (at, tag);
        }
        paired_search = at.checked_add(1)?;
    };
    let mut matched_tail = None;
    for tail in paired_byte_offset
        .checked_sub(15)
        .into_iter()
        .flat_map(|last| annotation_byte_offset..last)
    {
        if bytes.get(tail) != Some(&1) || bytes.get(tail + 5..tail + 11) != Some(&[0; 6]) {
            continue;
        }
        let Some(governing_owner_record_index) = View::u32_le_at(bytes, tail + 1) else {
            continue;
        };
        let Some(governing_companion_record_index) =
            governed_owners.get(&governing_owner_record_index).copied()
        else {
            continue;
        };
        let Some(return_count) =
            View::u32_le_at(bytes, tail + 11).and_then(|count| usize::try_from(count).ok())
        else {
            continue;
        };
        if return_count > 64 {
            continue;
        }
        let mut cursor = tail + 15;

        let mut return_members = Vec::new();
        let mut valid = true;
        for _ in 0..return_count {
            if bytes.get(cursor) != Some(&1) || bytes.get(cursor + 5..cursor + 11) != Some(&[0; 6])
            {
                valid = false;
                break;
            }
            let Some(reference) =
                View::u32_le_at(bytes, cursor + 1).and_then(std::num::NonZeroU32::new)
            else {
                valid = false;
                break;
            };
            if !geometry_indices.contains(&reference.get()) {
                valid = false;
                break;
            }
            return_members.push(crate::records::identity::Located {
                value: reference,
                offset: u64_from_index(cursor + 1),
            });
            cursor += 11;
        }
        if !valid
            || bytes
                .get(cursor..paired_byte_offset)?
                .iter()
                .any(|byte| *byte != 0)
        {
            continue;
        }
        let mut operand_members = [0u32; 64];
        let mut operand_count = 0usize;
        for operand in &operands {
            if let Some(index) = operand.geometry_record_index {
                operand_members[operand_count] = index.get();
                operand_count += 1;
            }
        }
        let mut returned = [0u32; 64];
        for (slot, member) in returned.iter_mut().zip(&return_members) {
            *slot = member.value.get();
        }
        operand_members[..operand_count].sort_unstable_by(Ord::cmp);
        returned[..return_members.len()].sort_unstable_by(Ord::cmp);
        if operand_members[..operand_count] != returned[..return_members.len()] {
            continue;
        }
        if matched_tail.is_some() {
            return None;
        }
        matched_tail = Some((
            tail,
            governing_owner_record_index,
            governing_companion_record_index,
            return_members,
        ));
    }
    let (tail, governing_owner_record_index, governing_companion_record_index, return_members) =
        matched_tail?;
    if bytes.get(paired_byte_offset + 11..paired_byte_offset + 19) != Some(&[0; 8])
        || bytes.get(paired_byte_offset + 19) != Some(&1)
        || bytes.get(paired_byte_offset + 24..paired_byte_offset + 30) != Some(&[0; 6])
    {
        return None;
    }
    let owner_reference = View::u32_le_at(bytes, paired_byte_offset + 20)?;
    if !sketch_entities.contains(&owner_reference) {
        return None;
    }
    let annotation_bytes = bytes.get(annotation_byte_offset..tail)?.to_vec();
    let draft = crate::records::dimensions::DesignDimensionAnnotationFrameDraft {
        id: String::new(),
        companion_record_index,
        governing_companion_record_index,
        byte_offset: u64_from_index(start),
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        record_index,
        frame_length: u64::try_from(paired_byte_offset.checked_sub(start)?).ok()?,
        operands,
        entity_genesis,
        annotation_bytes,
        annotation_byte_offset: u64_from_index(annotation_byte_offset),
        governing_owner_record_index,
        governing_owner_reference_offset: u64_from_index(tail + 1),
        return_members,
        paired_class_tag: crate::design::decode::text::class_tag_from_view(paired_class_tag)
            .ok()?,
        paired_byte_offset: u64_from_index(paired_byte_offset),
        owner_reference,
        owner_reference_offset: u64_from_index(paired_byte_offset + 20),
    };
    DesignDimensionAnnotationFrame::try_new(draft).ok()
}

/// Stable Fusion type whose indexed records carry the older direct dimension
/// presentation geometry.
const DIMENSION_PRESENTATION_TYPE_GUID: &str = "6CCF41D5-40BE-48ED-A834-18F3EAED6C57";
/// Stable Fusion type whose indexed records carry the current direct
/// dimension presentation geometry.
const DIMENSION_PRESENTATION_V3_TYPE_GUID: &str = "8C780195-72C0-4a56-A911-E43AB14357F2";
/// Stable `EntityTracking` type used by a dimension presentation's paired
/// header.
const DIMENSION_PRESENTATION_PAIR_TYPE_GUID: &str = "90055C05-546C-4EE7-B3C9-3DD922AD0C9C";
/// Stable `Dimension` type used by a current dimension presentation's paired
/// header.
const DIMENSION_PRESENTATION_V3_PAIR_TYPE_GUID: &str = "855F0A64-6286-4FFC-B918-BC394F600107";

fn is_dimension_presentation_pair_type(type_guid: &str) -> bool {
    type_guid.eq_ignore_ascii_case(DIMENSION_PRESENTATION_PAIR_TYPE_GUID)
        || type_guid.eq_ignore_ascii_case(DIMENSION_PRESENTATION_V3_PAIR_TYPE_GUID)
}

fn is_dimension_presentation_type(type_guid: &str) -> bool {
    type_guid.eq_ignore_ascii_case(DIMENSION_PRESENTATION_TYPE_GUID)
        || type_guid.eq_ignore_ascii_case(DIMENSION_PRESENTATION_V3_TYPE_GUID)
}

/// Decode direct presentation frames that precede a dimension parameter's
/// owner. The type table selects the primary and paired classes; no numeric
/// class tag is treated as a cross-stream type identity.
pub(crate) fn decode_dimension_presentation_frames(
    inputs: &DimensionDecodeInputs<'_>,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignDimensionPresentationFrame>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        placements,
        parameters,
        owners,
        points,
        curves,
        ..
    } = inputs;
    let parameter_kinds = dimension_parameter_index(parameters);
    let mut sketch_scope_by_entity = HashMap::new();
    for placement in placements {
        let Some(stream) = native_stream(&placement.id) else {
            continue;
        };
        let Some(scope_record_index) = placement.scope_record_index else {
            continue;
        };

        sketch_scope_by_entity.insert((stream, placement.entity_id.suffix()), scope_record_index);
    }
    let types = decode_types(scan)?;
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream = crate::ids::native_scope(&entry.name);
        let stream_types = stream_types_by_class_tag(&types, &entry.name);
        let mut presentation_classes = HashMap::new();
        for (class_tag, design_type) in &stream_types {
            if is_dimension_presentation_type(design_type.type_guid.as_str()) {
                presentation_classes.insert(*class_tag, design_type.type_guid.as_str());
            }
        }
        if presentation_classes.is_empty() {
            continue;
        }
        let mut paired_classes = HashSet::new();
        for (class_tag, design_type) in &stream_types {
            if is_dimension_presentation_pair_type(design_type.type_guid.as_str()) {
                paired_classes.insert(u64::from(*class_tag));
            }
        }
        if paired_classes.is_empty() {
            continue;
        }
        let geometry_indices = dimension_geometry_indices(&stream, points, curves);
        let mut sketch_entities = HashSet::new();
        for entity in entities.iter().filter(|entity| {
            native_stream(&entity.id) == Some(stream.as_str()) && entity.in_sketch_module()
        }) {
            let Ok(index) = u32::try_from(entity.entity_id.suffix()) else {
                continue;
            };

            sketch_entities.insert(index);
        }
        let bytes = scan.entry_bytes(&entry.name)?;
        for header in indexed_record_offsets(bytes) {
            let start = header.offset;
            let Some(primary_type_guid) = presentation_classes.get(&header.class_tag.code()) else {
                continue;
            };
            let Some(parsed) = parse_dimension_presentation_frame(
                bytes,
                start,
                primary_type_guid,
                &geometry_indices,
                &sketch_entities,
                &paired_classes,
            ) else {
                continue;
            };
            let mut frame = parsed;
            let Some(owner) = owners
                .iter()
                .filter(|owner| {
                    native_stream(owner.id()) == Some(stream.as_str())
                        && parameter_kinds
                            .get(&(stream.as_str(), owner.parameter_record_index()))
                            .is_some_and(|parameter| {
                                parameter.kind() == DesignParameterKind::Dimension
                            })
                        && owner.byte_offset() > frame.paired_byte_offset
                        && sketch_scope_by_entity
                            .get(&(stream.as_str(), u64::from(frame.owner_reference)))
                            .is_some_and(|scope_record_index| {
                                owner.scope_record_index() == *scope_record_index
                            })
                })
                .min_by_key(|owner| owner.byte_offset())
            else {
                continue;
            };
            frame.id = design_record_id(
                &entry.name,
                ":design-dimension-presentation-frame#",
                frame.byte_offset,
            );
            frame.governing_owner_record_index = owner.record_index();
            frame.governing_parameter_record_index = owner.parameter_record_index();
            frame.governing_companion_record_index = owner.companion_record_index();

            out.push(frame);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn parse_dimension_presentation_frame(
    bytes: &[u8],
    start: usize,
    primary_type_guid: &str,
    geometry_indices: &HashSet<u32>,
    sketch_entities: &HashSet<u32>,
    paired_classes: &HashSet<u64>,
) -> Option<DesignDimensionPresentationFrame> {
    if !is_dimension_presentation_type(primary_type_guid) {
        return None;
    }
    let (class_tag, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
    if after_tag != start.checked_add(7)?
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
    {
        return None;
    }
    let record_index = View::u32_le_at(bytes, after_tag)?;
    let count = usize::try_from(View::u32_le_at(bytes, start + 20)?).ok()?;
    if !(1..=64).contains(&count) {
        return None;
    }
    let mut position = start.checked_add(24)?;

    let mut operands = Vec::new();
    for _ in 0..count {
        if bytes.get(position) != Some(&1)
            || bytes.get(position + 5..position + 11) != Some(&[0; 6])
        {
            return None;
        }
        let geometry_record_index =
            std::num::NonZeroU32::new(View::u32_le_at(bytes, position + 1)?)?;
        if !geometry_indices.contains(&geometry_record_index.get()) {
            return None;
        }
        operands.push(DesignDimensionPresentationOperand {
            geometry_record_index,
            geometry_reference_offset: u64::try_from(position + 1).ok()?,
            role: View::u32_le_at(bytes, position + 11)?,
            role_offset: u64::try_from(position + 11).ok()?,
        });
        position = position.checked_add(15)?;
    }
    let presentation_byte_offset = position;
    let mut paired_search = position;
    let (paired_byte_offset, paired_class_tag) = loop {
        let at = next_indexed_record_offset(bytes, paired_search)?;
        let (tag, after) = lp_ascii_filtered_view(bytes, at, 3..=3, u8::is_ascii_digit)?;
        if View::u32_le_at(bytes, after) == Some(record_index)
            && !tag.starts_with('0')
            && tag
                .parse::<u64>()
                .ok()
                .is_some_and(|class_tag| paired_classes.contains(&class_tag))
        {
            if bytes.get(at + 11..at + 19) != Some(&[0; 8]) || bytes.get(at + 19) != Some(&1) {
                return None;
            }
            break (at, tag);
        }
        paired_search = at.checked_add(1)?;
    };
    let owner_reference = View::u32_le_at(bytes, paired_byte_offset + 20)?;
    if !sketch_entities.contains(&owner_reference) {
        return None;
    }
    let presentation_bytes = bytes
        .get(presentation_byte_offset..paired_byte_offset)?
        .to_vec();
    Some(DesignDimensionPresentationFrame {
        id: String::new(),
        byte_offset: u64::try_from(start).ok()?,
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        record_index,
        frame_length: u64::try_from(paired_byte_offset.checked_sub(start)?).ok()?,
        operands,
        presentation_bytes: presentation_bytes.into(),
        presentation_byte_offset: u64::try_from(presentation_byte_offset).ok()?,
        paired_class_tag: crate::design::decode::text::class_tag_from_view(paired_class_tag)
            .ok()?,
        paired_byte_offset: u64::try_from(paired_byte_offset).ok()?,
        owner_reference,
        owner_reference_offset: u64::try_from(paired_byte_offset + 20).ok()?,
        governing_owner_record_index: 0,
        governing_parameter_record_index: 0,
        governing_companion_record_index: 0,
    })
}

/// Decode counted typed sketch loci nested immediately after dimensional
/// parameter-companion prefixes.
pub(crate) fn decode_dimension_locus_groups(
    inputs: &DimensionDecodeInputs<'_>,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignDimensionLocusGroup>, CodecError> {
    let &DimensionDecodeInputs {
        scan,
        parameters,
        owners,
        companions,
        scopes,
        headers,
        points,
        curves,
        ..
    } = inputs;
    let parameters = dimension_parameter_index(parameters);
    let dimension_companions = dimension_companion_keys(owners, &parameters);
    let mut out = Vec::new();
    for (companion, scope) in companions.iter().filter_map(|companion| {
        let scope = native_stream(companion.id())?;
        dimension_companions
            .contains(&(scope, companion.record_index()))
            .then_some((companion, scope))
    }) {
        let entry = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, scope);
        let Some(entry) = entry else {
            continue;
        };
        let geometry_indices = dimension_geometry_indices(scope, points, curves);
        let mut sketch_entities = HashSet::new();
        for entity in entities
            .iter()
            .filter(|entity| native_stream(&entity.id) == Some(scope) && entity.in_sketch_module())
        {
            let Ok(index) = u32::try_from(entity.entity_id.suffix()) else {
                continue;
            };

            sketch_entities.insert(index);
        }
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some((start, end)) = companion_owned_interval(
            companion,
            parameters.values().copied(),
            owners,
            scopes,
            headers,
            bytes.len(),
        ) else {
            continue;
        };
        let candidates = find_dimension_locus_groups(
            bytes,
            start,
            end,
            companion.record_index(),
            &geometry_indices,
            &sketch_entities,
        );
        for mut group in candidates {
            group.id = design_record_id(
                &entry.name,
                ":design-dimension-locus-group#",
                group.byte_offset,
            );

            out.push(group);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn find_dimension_locus_groups(
    bytes: &[u8],
    start: usize,
    end: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
    sketch_entities: &HashSet<u32>,
) -> Vec<DesignDimensionLocusGroup> {
    let parse = |at| {
        parse_dimension_locus_group(
            bytes,
            at,
            companion_record_index,
            geometry_indices,
            sketch_entities,
        )
        .filter(|group| group.next_byte_offset <= u64_from_index(end))
    };
    let mut candidates = Vec::new();
    if let Some(group) = parse(start) {
        candidates.push(group);
    }
    let mut position = start.checked_add(1);
    while let Some(at) = position.and_then(|position| next_indexed_record_offset(bytes, position)) {
        if at >= end {
            break;
        }
        if let Some(group) = parse(at) {
            candidates.push(group);
        }
        position = at.checked_add(1);
    }
    candidates.sort_by_key(|group| group.byte_offset);
    candidates.dedup_by_key(|group| group.byte_offset);
    candidates
}

pub(super) fn companion_owned_interval<'a>(
    companion: &DesignParameterCompanion,
    parameters: impl IntoIterator<Item = &'a DesignParameter>,
    owners: &[DesignParameterOwner],
    scopes: &[DesignParameterScope],
    headers: &[DesignRecordHeader],
    stream_length: usize,
) -> Option<(usize, usize)> {
    let native_scope = native_stream(companion.id())?;
    let owning_scope_record_index = owners
        .iter()
        .find(|owner| {
            native_stream(owner.id()) == Some(native_scope)
                && owner.record_index() == companion.owner_record_index()
        })
        .map(crate::records::parameters::DesignParameterOwner::scope_record_index);
    let mut foreign_scope_members = HashSet::new();
    for member in scopes
        .iter()
        .filter(|scope| {
            native_stream(&scope.id) == Some(native_scope)
                && Some(scope.record_index) != owning_scope_record_index
        })
        .flat_map(|scope| scope.reference_members().values().copied())
    {
        if !foreign_scope_members.contains(&member) {
            foreign_scope_members.insert(member);
        }
    }
    let start = companion
        .payload_byte_offset()
        .and_then(|offset| usize::try_from(offset).ok())?;
    let end = owners
        .iter()
        .filter(|owner| {
            native_stream(owner.id()) == Some(native_scope)
                && owner.byte_offset() > companion.byte_offset()
        })
        .filter_map(|owner| usize::try_from(owner.byte_offset()).ok())
        .chain(
            parameters
                .into_iter()
                .filter(|parameter| {
                    native_stream(&parameter.id) == Some(native_scope)
                        && parameter.byte_offset() > companion.byte_offset()
                })
                .filter_map(|parameter| usize::try_from(parameter.byte_offset()).ok()),
        )
        .chain(
            scopes
                .iter()
                .filter(|scope| {
                    native_stream(&scope.id) == Some(native_scope)
                        && scope.byte_offset() > companion.byte_offset()
                })
                .filter_map(|scope| usize::try_from(scope.byte_offset()).ok()),
        )
        .chain(
            headers
                .iter()
                .filter(|header| {
                    native_stream(&header.id) == Some(native_scope)
                        && header.byte_offset > companion.byte_offset()
                        && foreign_scope_members.contains(&header.record_index)
                })
                .filter_map(|header| usize::try_from(header.byte_offset).ok()),
        )
        .min()
        .unwrap_or(stream_length);
    (start <= end && end <= stream_length).then_some((start, end))
}

fn parse_dimension_locus_group(
    bytes: &[u8],
    start: usize,
    companion_record_index: u32,
    geometry_indices: &HashSet<u32>,
    sketch_entities: &HashSet<u32>,
) -> Option<DesignDimensionLocusGroup> {
    let (class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    if after_tag != start.checked_add(7)?
        || bytes.get(start + 11..start + 19) != Some(&[0; 8])
        || bytes.get(start + 19) != Some(&1)
    {
        return None;
    }
    let record_index = View::u32_le_at(bytes, start + 7)?;
    let count = usize::try_from(View::u32_le_at(bytes, start + 20)?).ok()?;
    if !(1..=64).contains(&count) {
        return None;
    }
    let mut position = start.checked_add(24)?;

    let mut geometry = Vec::new();
    for _ in 0..count {
        if bytes.get(position) != Some(&1)
            || bytes.get(position + 5..position + 11) != Some(&[0; 6])
        {
            return None;
        }
        let geometry_record_index = View::u32_le_at(bytes, position + 1)?;
        if !geometry_indices.contains(&geometry_record_index) {
            return None;
        }
        geometry.push((
            geometry_record_index,
            u64_from_index(position + 1),
            View::u32_le_at(bytes, position + 11)?,
            u64_from_index(position + 11),
        ));
        position = position.checked_add(15)?;
    }
    if bytes.get(position) != Some(&0)
        || bytes.get(position + 1) != Some(&1)
        || bytes.get(position + 6..position + 12) != Some(&[0; 6])
    {
        return None;
    }
    let owner_reference = View::u32_le_at(bytes, position + 2)?;
    if !sketch_entities.contains(&owner_reference) {
        return None;
    }
    let owner_reference_offset = u64_from_index(position + 2);
    let owner_role = View::u32_le_at(bytes, position + 12)?;
    let owner_role_offset = u64_from_index(position + 12);
    position = position.checked_add(16)?;
    let state = View::u32_le_at(bytes, position)?;
    let state_offset = u64_from_index(position);
    let return_count = usize::try_from(View::u32_le_at(bytes, position + 4)?).ok()?;
    if return_count != count {
        return None;
    }
    position = position.checked_add(8)?;

    let mut loci = Vec::new();
    for (geometry_record_index, geometry_reference_offset, role, role_offset) in geometry {
        if bytes.get(position) != Some(&1)
            || bytes.get(position + 5..position + 11) != Some(&[0; 6])
        {
            return None;
        }
        let record_index = View::u32_le_at(bytes, position + 1)?;
        if !geometry_indices.contains(&record_index) {
            return None;
        }
        loci.push(DesignDimensionLocus {
            geometry_record_index,
            geometry_reference_offset,
            role,
            role_offset,
            returned: crate::records::identity::Located {
                value: record_index,
                offset: u64_from_index(position + 1),
            },
        });
        position = position.checked_add(11)?;
    }
    if bytes.get(position) != Some(&0) {
        return None;
    }
    let next_byte_offset = position.checked_add(1)?;
    let (next_class_tag, next_after_tag) =
        lp_ascii_filtered_view(bytes, next_byte_offset, 0..=2000, u8::is_ascii_graphic)?;
    if next_after_tag != next_byte_offset.checked_add(7)? {
        return None;
    }
    Some(DesignDimensionLocusGroup {
        id: String::new(),
        companion_record_index,
        byte_offset: u64_from_index(start),
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        record_index,
        frame_length: u64::try_from(next_byte_offset.checked_sub(start)?).ok()?,
        loci,
        owner_reference,
        owner_reference_offset,
        owner_role,
        owner_role_offset,
        state,
        state_offset,
        next_class_tag: crate::design::decode::text::class_tag_from_view(next_class_tag).ok()?,
        next_record_index: View::u32_le_at(bytes, next_after_tag)?,
        next_byte_offset: u64_from_index(next_byte_offset),
    })
}

#[cfg(test)]
mod tests;
