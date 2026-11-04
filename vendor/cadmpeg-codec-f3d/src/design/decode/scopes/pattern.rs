// SPDX-License-Identifier: Apache-2.0
//! Exact rectangular and circular pattern constructions and their axes.

use cadmpeg_core::convert::{f64_from_index, truncate_f64_to_u32};
use cadmpeg_core::decode::u64_from_index;

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use crate::bytes::lp_ascii_filtered_view;
use crate::bytes::{f64s_at, finite_reals_at};
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::decode::text::relaxed_guid_end;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::records::feature::patterns;
use crate::records::feature::patterns::DesignCircularPatternConstruction;
use crate::records::feature::patterns::DesignRectangularPatternConstruction;
use crate::records::feature::patterns::DesignRectangularPatternInstances;
use crate::records::feature::patterns::RectangularPatternSearchCap;
use crate::records::feature::patterns::{
    MAX_RECTANGULAR_PATTERN_INSTANCES, MAX_RECTANGULAR_PATTERN_RECORD_BYTES,
    MAX_RECTANGULAR_PATTERN_SCANNED_BYTES, RECTANGULAR_PATTERN_RUN_WORK,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::{View, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

const EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8: f64 = 1.0e-8;

const EPS_SCOPES_SAME_TRANSFORM_BASIS_E10: f64 = 1.0e-10;

pub(super) fn exact_rectangular_pattern_construction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Result<Option<DesignRectangularPatternConstruction>, CodecError> {
    let parsed = (|| {
        if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::RectangularPattern) {
            return None;
        }
        let stream = native_stream(&scope.id)?;
        let mut lanes = parameter_owners.iter().filter(|owner| {
            native_stream(owner.id()) == Some(stream)
                && owner.scope_record_index() == scope.record_index
        });
        let mut ordered_lanes = [lanes.next()?, lanes.next()?, lanes.next()?, lanes.next()?];
        if lanes.next().is_some() {
            return None;
        }
        ordered_lanes.sort_by_key(|left| left.local_ordinal());
        let [u_count, v_count, u_extent, v_extent] = ordered_lanes;
        if [u_count, v_count, u_extent, v_extent]
            .iter()
            .enumerate()
            .any(|(ordinal, owner)| u32::try_from(ordinal) != Ok(owner.local_ordinal()))
        {
            return None;
        }
        let exact_count = |value: f64| {
            (value > 0.0 && value <= f64::from(u32::MAX) && value.fract() == 0.0)
                .then(|| truncate_f64_to_u32(value))
                .flatten()
        };
        let u_count_value = exact_count(u_count.evaluated_value().get())?;
        let v_count_value = exact_count(v_count.evaluated_value().get())?;
        let construction = DesignRectangularPatternConstruction::try_from(
            patterns::DesignRectangularPatternConstructionWire {
                u_count: u_count_value,
                v_count: v_count_value,
                u_extent: u_extent.evaluated_value().get(),
                v_extent: v_extent.evaluated_value().get(),
                owner_record_indices: [
                    u_count.record_index(),
                    v_count.record_index(),
                    u_extent.record_index(),
                    v_extent.record_index(),
                ],
                value_offsets: [
                    u_count.evaluated_value_offset(),
                    v_count.evaluated_value_offset(),
                    u_extent.evaluated_value_offset(),
                    v_extent.evaluated_value_offset(),
                ],
                instances: None,
                instance_search_cap: None,
                directions: Vec::new(),
                secondary_direction: None,
            },
        )
        .ok()?;
        Some(construction)
    })();
    let Some(mut construction) = parsed else {
        return Ok(None);
    };
    let directions = exact_rectangular_pattern_directions(bytes, records, scope);
    let secondary = exact_rectangular_pattern_secondary_direction(
        bytes,
        records,
        scope,
        &construction,
        &directions,
    );
    construction
        .try_set_secondary_direction(secondary)
        .map_err(CodecError::malformed)?;
    if directions.len() + usize::from(construction.secondary_direction().is_some())
        == construction.active_direction_count()
    {
        construction
            .try_set_directions(directions)
            .map_err(CodecError::malformed)?;
    }
    if construction
        .directions()
        .first()
        .is_none_or(|record| record.distance_type != 7)
    {
        match exact_rectangular_pattern_instances(bytes, records, scope, &construction) {
            InstanceSearch::Resolved(instances) => construction
                .try_set_instances(Some(*instances))
                .map_err(CodecError::malformed)?,
            InstanceSearch::Unresolved => {}
            InstanceSearch::Capped(cap) => construction
                .mark_instance_search_capped(cap)
                .map_err(CodecError::malformed)?,
        }
    }
    Ok(Some(construction))
}

/// The direction records of a rectangular-pattern scope, in reference order.
fn exact_rectangular_pattern_directions(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
) -> Vec<patterns::DesignRectangularPatternDirection> {
    let mut directions = Vec::new();
    for (record_index, selection_record_index) in scope
        .reference_members()
        .values()
        .zip(scope.reference_members().values().skip(1))
    {
        for (start, paired_at) in records.frames(*record_index) {
            if let Some(direction) = exact_rectangular_pattern_direction(
                bytes,
                start,
                paired_at,
                *record_index,
                *selection_record_index,
                scope.record_index,
            ) {
                directions.push(direction);
            }
        }
    }
    directions
}

fn exact_rectangular_pattern_direction(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    record_index: u32,
    selection_record_index: u32,
    scope_record_index: u32,
) -> Option<patterns::DesignRectangularPatternDirection> {
    use crate::layout::rectangular_pattern_direction_record as frame;

    use crate::layout::rectangular_pattern_spacing_direction_record as spacing;

    let distance_type = View::u32_le_at(bytes, start + frame::FIXED_EIGHT)?;
    let shift = match (paired_at.checked_sub(start)?, distance_type) {
        (frame::LEN, 8) => 0,
        (spacing::LEN, 7) if bytes.get(start + spacing::SPACING_FLAG) == Some(&0) => 1,
        _ => return None,
    };
    let header = start + frame::INDEXED_HEADER;
    let (class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, header, 0..=2000, u8::is_ascii_graphic)?;
    let selection_kind = View::u32_le_at(bytes, start + frame::SELECTION_KIND)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || after_tag != header + 7
        || View::u32_le_at(bytes, after_tag) != Some(record_index)
        || bytes.get(start + frame::ZERO_RUN_10..start + frame::SELECTION_KIND) != Some(&[0; 10])
        || !matches!(selection_kind, 8 | 16)
        || bytes.get(start + frame::ZERO_RUN_16..start + frame::FIXED_EIGHT) != Some(&[0; 16])
        || (0..3).any(|lane| {
            View::f64_le_at(bytes, start + shift + frame::NEGATIVE_ONES + lane * 8) != Some(-1.0)
        })
        || View::u32_le_at(bytes, start + shift + frame::SELECTION_COUNT) != Some(1)
        || marked_record_reference(bytes, start + shift + frame::SELECTION_REFERENCE)
            != Some(selection_record_index)
        || bytes.get(start + shift + frame::ZERO_RUN_2..start + shift + frame::AUXILIARY_COUNT)
            != Some(&[0; 2])
        || View::u32_le_at(bytes, start + shift + frame::AUXILIARY_COUNT) != Some(1)
        || marked_record_reference(bytes, start + shift + frame::AUXILIARY_REFERENCE).is_none()
        || View::u64_le_at(bytes, start + shift + frame::ROLE) != Some(0x0000_0004_0000_0000)
        || bytes
            .get(start + shift + frame::ZERO_RUN_AFTER_ROLE..start + shift + frame::OPAQUE_INDEX)
            != Some(&[0; 10])
    {
        return None;
    }
    let opaque_index = View::u32_le_at(bytes, start + shift + frame::OPAQUE_INDEX)?;
    if opaque_index == 0
        || !View::f64_le_at(bytes, start + shift + frame::OPAQUE_SCALAR)?.is_finite()
        || View::u32_le_at(bytes, start + shift + frame::OPAQUE_INDEX_REPEAT) != Some(opaque_index)
        || marked_record_reference(bytes, start + shift + frame::INDEX_PLUS_TWO_REFERENCE)
            != record_index.checked_add(2)
        || bytes.get(start + shift + frame::SEPARATOR_FLAG) != Some(&1)
        || bytes.get(
            start + shift + frame::SEPARATOR_ZERO_RUN
                ..start + shift + frame::INDEX_PLUS_ONE_REFERENCE,
        ) != Some(&[0; 2])
        || marked_record_reference(bytes, start + shift + frame::INDEX_PLUS_ONE_REFERENCE)
            != record_index.checked_add(1)
        || bytes.get(start + shift + frame::SCOPE_SEPARATOR) != Some(&0)
        || marked_record_reference(bytes, start + shift + frame::SCOPE_REFERENCE)
            != Some(scope_record_index)
    {
        return None;
    }
    let (paired_class_tag, paired_after_tag) =
        lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    if paired_class_tag.len() != 3
        || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || paired_after_tag != paired_at + 7
        || View::u32_le_at(bytes, paired_after_tag) != Some(record_index)
    {
        return None;
    }
    let direction = patterns::DesignRectangularPatternDirection {
        record_index,
        selection_record_index,
        selection_kind,
        distance_type,
        origin: finite_reals_at(bytes, start + frame::ORIGIN)?,
        origin_offset: u64::try_from(start.checked_add(frame::ORIGIN)?).ok()?,
        displacement: finite_reals_at(bytes, start + frame::DISPLACEMENT)?,
        displacement_offset: u64::try_from(start.checked_add(frame::DISPLACEMENT)?).ok()?,
    };
    direction.direction().is_some().then_some(direction)
}

fn exact_rectangular_pattern_secondary_direction(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    construction: &DesignRectangularPatternConstruction,
    directions: &[patterns::DesignRectangularPatternDirection],
) -> Option<crate::records::identity::Located<[FiniteReal; 3]>> {
    use crate::layout::rectangular_pattern_payload_prefix as prefix;
    use crate::layout::rectangular_pattern_payload_suffix as suffix;

    let [direction] = directions else {
        return None;
    };
    if construction.active_direction_count() != 2 || direction.distance_type != 7 {
        return None;
    }
    let mut found = None;
    for (_, offsets) in records.records() {
        for &start in offsets {
            let owners = construction.owner_record_indices;
            if bytes.get(start + prefix::ZERO_HEAD..start + prefix::FLAG) != Some(&[0; 8])
                || bytes.get(start + prefix::FLAG) != Some(&1)
                || View::u32_le_at(bytes, start + prefix::COUNT) != Some(2)
                || bytes.get(start + prefix::ZERO_MIDDLE..start + prefix::V_COUNT) != Some(&[0; 4])
                || bytes.get(start + prefix::ZERO_TAIL..start + prefix::SENTINEL) != Some(&[0; 9])
                || View::u32_le_at(bytes, start + prefix::SENTINEL) != Some(u32::MAX)
                || bytes.get(start + prefix::ZERO_DISTANCE..start + prefix::V_COUNT_REPEAT)
                    != Some(&[0; 2])
                || [
                    (prefix::U_COUNT, owners[0]),
                    (prefix::V_COUNT, owners[1]),
                    (prefix::U_DISTANCE, owners[2]),
                    (prefix::V_DISTANCE, owners[3]),
                    (prefix::V_COUNT_REPEAT, owners[1]),
                    (prefix::U_DIRECTION, direction.record_index),
                ]
                .into_iter()
                .any(|(offset, index)| {
                    marked_record_reference(bytes, start + offset) != Some(index)
                })
            {
                continue;
            }
            let end = next_indexed_record_offset(bytes, start + prefix::LEN)?;
            if end.checked_sub(start)? < prefix::LEN + suffix::LEN {
                continue;
            }
            let tail = end - suffix::LEN;
            let seed_group = marked_record_reference(bytes, tail + suffix::SEED_GROUP)?;
            if marked_record_reference(bytes, tail + suffix::U_COUNT) != Some(owners[0])
                || marked_record_reference(bytes, tail + suffix::SCOPE) != Some(scope.record_index)
                || !scope
                    .reference_members()
                    .values()
                    .any(|index| *index == seed_group)
                || bytes.get(tail + suffix::ZERO_TAIL..end) != Some(&[0; 4])
            {
                continue;
            }
            let Some(value) = finite_reals_at::<3>(bytes, start + prefix::V_DIRECTION) else {
                continue;
            };
            cadmpeg_ir::units::UnitVector3::normalized(cadmpeg_ir::math::Vector3::from(
                value.map(FiniteReal::get),
            ))?;
            let candidate = crate::records::identity::Located {
                value,
                offset: u64::try_from(start + prefix::V_DIRECTION).ok()?,
            };
            if found.replace(candidate).is_some() {
                return None;
            }
        }
    }
    found
}

/// Outcome of the rectangular-pattern instance search.
#[derive(Debug, PartialEq)]
enum InstanceSearch {
    /// One exact instance run.
    Resolved(Box<DesignRectangularPatternInstances>),
    /// No unique exact instance run.
    Unresolved,
    /// The search stopped at a local cap.
    Capped(RectangularPatternSearchCap),
}

fn exact_rectangular_pattern_instances(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    construction: &DesignRectangularPatternConstruction,
) -> InstanceSearch {
    (|| {
        let mut active = [
            (construction.u_count(), construction.u_extent()),
            (construction.v_count(), construction.v_extent()),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 1);
        let (count, extent) = active.next()?;
        if active.next().is_some() {
            return None;
        }
        let count = usize::try_from(count).ok()?;
        if count > MAX_RECTANGULAR_PATTERN_INSTANCES {
            return Some(InstanceSearch::Capped(
                RectangularPatternSearchCap::Instances,
            ));
        }
        if scope.reference_members().len() != count.checked_add(6)?
            || !scope
                .reference_members()
                .values()
                .skip(1)
                .take(4)
                .eq(construction.owner_record_indices.iter())
        {
            return None;
        }

        let mut record_indices = Vec::new();
        record_indices.push(*scope.reference_members().values().next()?);
        record_indices.extend(
            scope
                .reference_members()
                .values_in(6..count.checked_add(5)?)?
                .copied(),
        );

        let mut reference_starts = Vec::new();
        for record_index in scope.reference_members().values() {
            reference_starts.push((*record_index, records.first_at_or_after(0, *record_index)?));
        }

        let mut candidates = Vec::new();
        let mut scanned_bytes = 0_usize;
        for record_index in &record_indices {
            let start = reference_starts
                .iter()
                .find_map(|(candidate, offset)| (candidate == record_index).then_some(*offset))?;
            let end = reference_starts
                .iter()
                .filter_map(|(_, offset)| (*offset > start).then_some(*offset))
                .min()?;
            let span = end.checked_sub(start)?;
            if span > MAX_RECTANGULAR_PATTERN_RECORD_BYTES {
                return Some(InstanceSearch::Capped(
                    RectangularPatternSearchCap::RecordBytes,
                ));
            }
            let Some(total) = scanned_bytes
                .checked_add(span)
                .filter(|total| *total <= MAX_RECTANGULAR_PATTERN_SCANNED_BYTES)
            else {
                return Some(InstanceSearch::Capped(
                    RectangularPatternSearchCap::ScannedBytes,
                ));
            };
            scanned_bytes = total;
            candidates.push(exact_rigid_transform_candidates(bytes, start, end)?);
        }
        let first_candidates = candidates.first()?;
        let final_candidates = candidates.last()?;
        let budget = WorkBudget::new(RECTANGULAR_PATTERN_RUN_WORK);
        let mut runs = Vec::new();
        for first in first_candidates {
            for final_candidate in final_candidates {
                if !budget.charge() {
                    return Some(InstanceSearch::Capped(RectangularPatternSearchCap::RunWork));
                }
                if !same_transform_basis(&first.0, &final_candidate.0) {
                    continue;
                }
                let delta = translation_delta(&first.0, &final_candidate.0);
                let distance = cadmpeg_ir::math::Vector3::from(delta).norm();
                if (distance - extent.abs()).abs()
                    > EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8
                {
                    continue;
                }

                let mut run = Vec::new();
                run.push(*first);
                let mut unique = true;
                for (ordinal, record_candidates) in candidates[1..count - 1].iter().enumerate() {
                    let fraction = f64_from_index(ordinal + 1)? / f64_from_index(count - 1)?;
                    let mut matched = None;
                    for candidate in record_candidates {
                        if !budget.charge() {
                            return Some(InstanceSearch::Capped(
                                RectangularPatternSearchCap::RunWork,
                            ));
                        }
                        if same_transform_basis(&first.0, &candidate.0)
                            && translation_delta(&first.0, &candidate.0)
                                .iter()
                                .zip(delta)
                                .all(|(value, total)| {
                                    (*value - total * fraction).abs()
                                        <= EPS_SCOPES_EXACT_RECTANGULAR_PATTERN_INSTANCES_E8
                                })
                        {
                            if matched.is_some() {
                                unique = false;
                                break;
                            }
                            matched = Some(candidate);
                        }
                    }
                    let Some(candidate) = matched else {
                        unique = false;
                        break;
                    };
                    if !unique {
                        break;
                    }
                    run.push(*candidate);
                }
                if unique {
                    run.push(*final_candidate);
                    runs.push(run);
                }
            }
        }
        runs.sort_by(|a, b| {
            a.iter()
                .map(|(_, offset)| *offset)
                .cmp(b.iter().map(|(_, offset)| *offset))
        });
        runs.dedup_by(|left, right| left == right);
        let [run] = runs.as_slice() else {
            return None;
        };

        let mut instances = Vec::new();
        for (record_index, (value, offset)) in record_indices.into_iter().zip(run) {
            instances.push(patterns::DesignPatternInstance {
                record_index,
                transform: crate::records::identity::Located {
                    value: *value,
                    offset: *offset,
                },
            });
        }
        Some(InstanceSearch::Resolved(Box::new(
            DesignRectangularPatternInstances::Bodies(instances),
        )))
    })()
    .unwrap_or(InstanceSearch::Unresolved)
}

type TransformCandidate = (crate::records::sketch_placement::SketchPlacementMatrix, u64);

fn exact_rigid_transform_candidates(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> Option<Vec<TransformCandidate>> {
    /// The single byte image of `1.0_f64`, the last lane of a rigid
    /// transform's fixed `0 0 0 1` bottom row.
    const ONE_F64_LE: [u8; 8] = [0, 0, 0, 0, 0, 0, 0xF0, 0x3F];
    let last_exclusive = end.checked_sub(127)?;
    if start >= last_exclusive || end > bytes.len() {
        // An exhaustive scan over this range finds nothing or aborts on its
        // first out-of-bounds sixteen-lane read.
        return None;
    }
    let mut candidates = Vec::new();
    // A valid transform carries exactly `1.0` at lane fifteen and a zero of
    // either sign at lanes twelve to fourteen. Locate the fixed `1.0` image
    // (it cannot overlap itself, so every occurrence surfaces) and read the
    // full sixteen-lane frame only at surviving offsets.
    for hit in memchr::memmem::find_iter(&bytes[start + 120..end], &ONE_F64_LE) {
        let offset = start + hit;
        let zero_lanes_valid = (0..3).all(|lane| {
            let at = offset + 96 + lane * 8;
            bytes[at..at + 7].iter().all(|byte| *byte == 0)
                && (bytes[at + 7] == 0 || bytes[at + 7] == 0x80)
        });
        if !zero_lanes_valid {
            continue;
        }
        let values = f64s_at::<16>(bytes, offset)?;
        let mut transform = [[0.0; 4]; 4];
        for (ordinal, value) in values.into_iter().enumerate() {
            transform[ordinal / 4][ordinal % 4] = value;
        }
        if let Ok(transform) =
            crate::records::sketch_placement::SketchPlacementMatrix::try_from(transform)
        {
            candidates.push((transform, u64::try_from(offset).ok()?));
        }
    }
    (!candidates.is_empty()).then_some(candidates)
}

fn same_transform_basis(
    left: &crate::records::sketch_placement::SketchPlacementMatrix,
    right: &crate::records::sketch_placement::SketchPlacementMatrix,
) -> bool {
    (0..3).all(|row| {
        (0..3).all(|column| {
            (left[row][column] - right[row][column]).abs() <= EPS_SCOPES_SAME_TRANSFORM_BASIS_E10
        })
    })
}

fn translation_delta(
    left: &crate::records::sketch_placement::SketchPlacementMatrix,
    right: &crate::records::sketch_placement::SketchPlacementMatrix,
) -> [f64; 3] {
    [
        right[0][3] - left[0][3],
        right[1][3] - left[1][3],
        right[2][3] - left[2][3],
    ]
}

pub(super) fn exact_circular_pattern_construction_with_owners(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[crate::records::parameters::DesignParameterOwner],
) -> Result<Option<DesignCircularPatternConstruction>, CodecError> {
    let parsed = (|| {
        if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::CircularPattern) {
            return None;
        }
        let mut axis_candidates = Vec::new();
        for (record_index, selection_record_index) in scope
            .reference_members()
            .values()
            .zip(scope.reference_members().values().skip(1))
        {
            for (start, paired_at) in records.frames(*record_index) {
                if let Some(axis) = exact_circular_pattern_axis(
                    bytes,
                    start,
                    paired_at,
                    *record_index,
                    *selection_record_index,
                    scope.record_index,
                ) {
                    axis_candidates.push(CircularPatternAxisCandidate {
                        axis,
                        axis_record_index: *record_index,
                        selection_record_index: *selection_record_index,
                    });
                }
            }
        }
        for record_index in scope.reference_members().values() {
            for (start, paired_at) in records.frames(*record_index) {
                let candidate = match exact_legacy_circular_pattern_axis(
                    bytes,
                    records,
                    start,
                    paired_at,
                    *record_index,
                    scope,
                ) {
                    Ok(candidate) => candidate,
                    Err(error) => return Some(Err(error)),
                };
                if let Some((axis, selection_record_index)) = candidate {
                    axis_candidates.push(CircularPatternAxisCandidate {
                        axis,
                        axis_record_index: *record_index,
                        selection_record_index,
                    });
                }
            }
        }
        let axis_index = select_circular_pattern_axis(&axis_candidates)?;
        let CircularPatternAxisCandidate {
            axis,
            axis_record_index,
            selection_record_index,
        } = axis_candidates.into_iter().nth(axis_index)?;
        let owner_count_candidates = parameter_owners.iter().filter_map(|owner| {
            if native_stream(owner.id()) != native_stream(&scope.id)
                || owner.scope_record_index() != scope.record_index
                || owner.local_ordinal() != 0
                || owner.evaluated_value().get() <= 0.0
                || owner.evaluated_value().get() > f64::from(u32::MAX)
                || owner.evaluated_value().get().fract() != 0.0
            {
                return None;
            }
            Some((
                truncate_f64_to_u32(owner.evaluated_value().get())?,
                owner.record_index(),
                owner.evaluated_value_offset(),
            ))
        });
        let mut count_candidates = Vec::new();
        for candidate in owner_count_candidates {
            count_candidates.push(candidate);
        }
        if count_candidates.is_empty() {
            for record_index in scope.reference_members().values() {
                if let Some((count, count_offset)) =
                    exact_fixed_pattern_count(bytes, records, *record_index, scope.record_index)
                {
                    count_candidates.push((count, *record_index, count_offset));
                }
            }
        }
        count_candidates.sort_unstable_by(Ord::cmp);
        count_candidates.dedup();
        let [(count, count_record_index, count_offset)] = count_candidates.as_slice() else {
            return None;
        };
        let owner_angle_candidates = parameter_owners.iter().filter_map(|owner| {
            (native_stream(owner.id()) == native_stream(&scope.id)
                && owner.scope_record_index() == scope.record_index
                && owner.local_ordinal() == 1)
                .then_some(())?;
            Some((
                cadmpeg_ir::scalar::PositiveAngle::new(owner.evaluated_value().get())?,
                owner.record_index(),
                owner.evaluated_value_offset(),
            ))
        });
        let mut angle_candidates = Vec::new();
        for candidate in owner_angle_candidates {
            angle_candidates.push(candidate);
        }
        if angle_candidates.is_empty() {
            for record_index in scope.reference_members().values() {
                let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index) else {
                    continue;
                };
                if scalar.owner_record_index != Some(scope.record_index) || scalar.ordinal != 1 {
                    continue;
                }
                let Some(angle) = cadmpeg_ir::scalar::PositiveAngle::new(scalar.value.get()) else {
                    continue;
                };

                angle_candidates.push((angle, *record_index, scalar.value_offset));
            }
        }
        angle_candidates.sort_by(|left, right| {
            left.0
                .get()
                .total_cmp(&right.0.get())
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        });
        angle_candidates.dedup();
        let [(angle, angle_record_index, angle_offset)] = angle_candidates.as_slice() else {
            return None;
        };
        Some(Ok(DesignCircularPatternConstruction {
            count: std::num::NonZeroU32::new(*count)?,
            count_record_index: *count_record_index,
            count_offset: *count_offset,
            angle: *angle,
            angle_record_index: *angle_record_index,
            angle_offset: *angle_offset,
            axis,
            axis_record_index,
            selection_record_index,
        }))
    })();
    parsed.transpose()
}

/// Circular pattern axis with its carrier and selection record indices.
struct CircularPatternAxisCandidate {
    axis: patterns::DesignCircularPatternAxis,
    axis_record_index: u32,
    selection_record_index: u32,
}

/// Select one circular-pattern axis, preferring the explicit solved carrier.
fn select_circular_pattern_axis(candidates: &[CircularPatternAxisCandidate]) -> Option<usize> {
    let mut inline = candidates.iter().enumerate().filter(|(_, candidate)| {
        matches!(
            candidate.axis,
            patterns::DesignCircularPatternAxis::Inline { .. }
        )
    });
    match (inline.next(), inline.next()) {
        (Some((index, _)), None) => Some(index),
        (None, None) => match candidates {
            [_] => Some(0),
            _ => None,
        },
        _ => None,
    }
}

fn exact_legacy_circular_pattern_axis(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    start: usize,
    paired_at: usize,
    record_index: u32,
    scope: &DesignParameterScope,
) -> Result<Option<(patterns::DesignCircularPatternAxis, u32)>, CodecError> {
    let parsed = (|| {
        use patterns::DesignCircularPatternAxis;

        let (class_tag, after_tag) =
            lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
        if class_tag.len() != 3
            || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
            || after_tag != start + 7
            || View::u32_le_at(bytes, after_tag) != Some(record_index)
            || bytes.get(start + 11..start + 21) != Some(&[0; 10])
        {
            return None;
        }
        let (
            identity_offsets,
            selection_at,
            second_count_at,
            second_identity_at,
            scope_at,
            tail_at,
        ) = match (
            paired_at.checked_sub(start),
            View::u32_le_at(bytes, start + 21),
        ) {
            (Some(129), Some(1)) => (
                [Some(start + 26), Some(start + 56)],
                start + 40,
                start + 51,
                start + 55,
                start + 66,
                start + 77,
            ),
            (Some(118), Some(0)) => (
                [Some(start + 45), None],
                start + 29,
                start + 40,
                start + 44,
                start + 55,
                start + 66,
            ),
            _ => return None,
        };
        let first_identity_at = identity_offsets
            .first()
            .copied()
            .flatten()?
            .checked_sub(1)?;
        if (identity_offsets[1].is_some()
            && (marked_record_reference(bytes, first_identity_at).is_none()
                || bytes.get(first_identity_at + 5..first_identity_at + 11) != Some(&[0; 6])
                || View::u32_le_at(bytes, start + 36) != Some(1)))
            || View::u32_le_at(bytes, second_count_at) != Some(1)
            || marked_record_reference(bytes, second_identity_at).is_none()
            || bytes.get(second_identity_at + 5..second_identity_at + 11) != Some(&[0; 6])
            || marked_record_reference(bytes, scope_at) != Some(scope.record_index)
            || bytes.get(scope_at + 5..scope_at + 11) != Some(&[0; 6])
        {
            return None;
        }
        let selection_record_index = marked_record_reference(bytes, selection_at)?;
        if !scope
            .reference_members()
            .values()
            .any(|value| value == &selection_record_index)
            || bytes.get(selection_at + 5..selection_at + 11) != Some(&[0; 6])
        {
            return None;
        }
        let opaque_index = View::u32_le_at(bytes, tail_at)?;
        if opaque_index == 0
            || !View::f64_le_at(bytes, tail_at + 4)?.is_finite()
            || View::u32_le_at(bytes, tail_at + 12) != Some(opaque_index)
            || marked_record_reference(bytes, tail_at + 16) != record_index.checked_add(2)
            || bytes.get(tail_at + 21..tail_at + 27) != Some(&[0; 6])
            || bytes.get(tail_at + 27..tail_at + 29) != Some(&[0; 2])
            || marked_record_reference(bytes, tail_at + 29) != record_index.checked_add(1)
            || bytes.get(tail_at + 34..tail_at + 40) != Some(&[0; 6])
            || bytes.get(tail_at + 40) != Some(&0)
            || marked_record_reference(bytes, tail_at + 41) != Some(scope.record_index)
            || bytes.get(tail_at + 46..tail_at + 52) != Some(&[0; 6])
        {
            return None;
        }
        let (paired_class_tag, paired_after_tag) =
            lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
        if paired_class_tag.len() != 3
            || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
            || paired_after_tag != paired_at + 7
            || View::u32_le_at(bytes, paired_after_tag) != Some(record_index)
        {
            return None;
        }
        let mut wrappers = [None, None];
        for (slot, offset) in wrappers
            .iter_mut()
            .zip(identity_offsets.into_iter().flatten())
        {
            let record_index = View::u32_le_at(bytes, offset)?;
            let (identity, identity_offset) =
                exact_pattern_identity_wrapper(bytes, records, record_index)?;
            *slot = Some((
                identity,
                patterns::DesignPatternAxisWrapper {
                    record_index,
                    identity_offset,
                },
            ));
        }
        let persistent_identity = wrappers.first()?.as_ref()?.0;
        if wrappers
            .iter()
            .flatten()
            .any(|(identity, _)| *identity != persistent_identity)
        {
            return None;
        }

        let mut retained_wrappers = Vec::new();
        for (_, wrapper) in wrappers.into_iter().flatten() {
            retained_wrappers.push(wrapper);
        }
        Some(Ok((
            DesignCircularPatternAxis::HistoricalEdge {
                wrappers: retained_wrappers.try_into().ok()?,
                persistent_identity,
                resolved: None,
            },
            selection_record_index,
        )))
    })();
    parsed.transpose()
}
fn exact_pattern_identity_wrapper(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
) -> Option<(u64, u64)> {
    let [start] = records.offsets(record_index) else {
        return None;
    };
    let start = *start;
    let (_, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
    if after_tag != start + 7
        || View::u32_le_at(bytes, after_tag) != Some(record_index)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
        || View::u64_le_at(bytes, start + 21)? == 0
    {
        return None;
    }
    let after_asset_id = relaxed_guid_end(bytes, start + 29)?;
    let after_context_id = relaxed_guid_end(bytes, after_asset_id)?;
    if View::u32_le_at(bytes, after_context_id) != Some(2)
        || bytes.get(after_context_id + 4..after_context_id + 8) != Some(&[0; 4])
        || marked_record_reference(bytes, after_context_id + 8) != record_index.checked_add(1)
        || bytes.get(after_context_id + 13..after_context_id + 19) != Some(&[0; 6])
    {
        return None;
    }
    let nested_one_at = next_indexed_record_offset(bytes, after_context_id + 19)?;
    let (_, nested_one_tag) =
        lp_ascii_filtered_view(bytes, nested_one_at, 3..=3, u8::is_ascii_digit)?;
    if View::u32_le_at(bytes, nested_one_tag) != record_index.checked_add(1)
        || bytes.get(nested_one_at + 11..nested_one_at + 21) != Some(&[0; 10])
        || marked_record_reference(bytes, nested_one_at + 21) != record_index.checked_add(2)
        || bytes.get(nested_one_at + 26..nested_one_at + 32) != Some(&[0; 6])
    {
        return None;
    }
    let identity_at = next_indexed_record_offset(bytes, nested_one_at + 32)?;
    let (_, identity_tag) = lp_ascii_filtered_view(bytes, identity_at, 3..=3, u8::is_ascii_digit)?;
    let next_at = next_indexed_record_offset(bytes, identity_at + 29)?;
    let (_, next_tag) = lp_ascii_filtered_view(bytes, next_at, 3..=3, u8::is_ascii_digit)?;
    if View::u32_le_at(bytes, identity_tag) != record_index.checked_add(2)
        || bytes.get(identity_at + 11..identity_at + 21) != Some(&[0; 10])
        || identity_at.checked_add(29) != Some(next_at)
        || View::u32_le_at(bytes, next_tag) != record_index.checked_add(3)
    {
        return None;
    }
    Some((
        View::u64_le_at(bytes, identity_at + 21)?,
        u64::try_from(identity_at + 21).ok()?,
    ))
}

fn exact_circular_pattern_axis(
    bytes: &[u8],
    start: usize,
    paired_at: usize,
    record_index: u32,
    selection_record_index: u32,
    scope_record_index: u32,
) -> Option<patterns::DesignCircularPatternAxis> {
    let (class_tag, after_tag) =
        lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
    if class_tag.len() != 3
        || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || after_tag != start + 7
        || View::u32_le_at(bytes, after_tag) != Some(record_index)
        || paired_at.checked_sub(start) != Some(195)
        || bytes.get(start + 11..start + 21) != Some(&[0; 10])
        || View::u32_le_at(bytes, start + 21) != Some(8)
        || bytes.get(start + 73..start + 89) != Some(&[0; 16])
        || View::u32_le_at(bytes, start + 89)? == 0
        || View::u32_le_at(bytes, start + 93) != Some(1)
        || marked_record_reference(bytes, start + 97) != Some(selection_record_index)
        || bytes.get(start + 102..start + 108) != Some(&[0; 6])
        || bytes.get(start + 108..start + 110) != Some(&[0; 2])
        || View::u32_le_at(bytes, start + 110) != Some(1)
        || marked_record_reference(bytes, start + 114).is_none()
        || bytes.get(start + 119..start + 125) != Some(&[0; 6])
        || View::u64_le_at(bytes, start + 125) != Some(0x0000_0004_0000_0000)
        || bytes.get(start + 133..start + 143) != Some(&[0; 10])
    {
        return None;
    }
    let opaque_index = View::u32_le_at(bytes, start + 143)?;
    if opaque_index == 0
        || !View::f64_le_at(bytes, start + 147)?.is_finite()
        || View::u32_le_at(bytes, start + 155) != Some(opaque_index)
        || marked_record_reference(bytes, start + 159) != record_index.checked_add(2)
        || bytes.get(start + 164..start + 172) != Some(&[0; 8])
        || marked_record_reference(bytes, start + 172) != record_index.checked_add(1)
        || bytes.get(start + 177..start + 184) != Some(&[0; 7])
        || marked_record_reference(bytes, start + 184) != Some(scope_record_index)
        || bytes.get(start + 189..start + 195) != Some(&[0; 6])
    {
        return None;
    }
    let (paired_class_tag, paired_after_tag) =
        lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    if paired_class_tag.len() != 3
        || !paired_class_tag.bytes().all(|byte| byte.is_ascii_digit())
        || paired_after_tag != paired_at + 7
        || View::u32_le_at(bytes, paired_after_tag) != Some(record_index)
    {
        return None;
    }
    let origin = finite_reals_at(bytes, start + 25)?;
    let displacement = finite_reals_at(bytes, start + 49)?.map(FiniteReal::get);
    patterns::DesignCircularPatternAxis::inline(
        origin,
        u64::try_from(start.checked_add(25)?).ok()?,
        displacement,
        u64::try_from(start.checked_add(49)?).ok()?,
    )
}

fn exact_fixed_pattern_count(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<(u32, u64)> {
    let mut candidates = records
        .frames(record_index)
        .filter_map(|(start, paired_at)| {
            let (class_tag, after_tag) =
                lp_ascii_filtered_view(bytes, start, 0..=2000, u8::is_ascii_graphic)?;
            if class_tag.len() != 3
                || !class_tag.bytes().all(|byte| byte.is_ascii_digit())
                || after_tag != start + 7
                || View::u32_le_at(bytes, after_tag) != Some(record_index)
                || paired_at.checked_sub(start) != Some(99)
                || bytes.get(start + 11..start + 19) != Some(&[0; 8])
                || bytes.get(start + 19) != Some(&1)
                || View::u32_le_at(bytes, start + 20) != Some(1)
                || marked_record_reference(bytes, start + 24) != Some(scope_record_index)
                || bytes.get(start + 29..start + 40) != Some(&[0; 11])
                || marked_record_reference(bytes, start + 44) != record_index.checked_add(2)
                || bytes.get(start + 49..start + 55) != Some(&[0; 6])
                || View::u32_le_at(bytes, start + 55)? == 0
                || bytes.get(start + 59..start + 63) != Some(&[0; 4])
                || marked_record_reference(bytes, start + 63) != Some(scope_record_index)
                || bytes.get(start + 68..start + 76) != Some(&[0; 8])
                || marked_record_reference(bytes, start + 76) != record_index.checked_add(1)
                || bytes.get(start + 81..start + 88) != Some(&[0; 7])
                || marked_record_reference(bytes, start + 88) != Some(scope_record_index)
                || bytes.get(start + 93..start + 99) != Some(&[0; 6])
            {
                return None;
            }
            let count = View::u32_le_at(bytes, start + 40)?;
            (count > 0).then_some((count, u64_from_index(start + 40)))
        });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

#[cfg(test)]
mod tests;
