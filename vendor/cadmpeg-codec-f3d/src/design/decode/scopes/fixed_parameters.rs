// SPDX-License-Identifier: Apache-2.0
//! Exact fixed extrude, fillet and chamfer parameter scopes.

use super::shared_frames::exact_fixed_scalar;
use super::shared_frames::marked_record_reference;
use super::shared_frames::FixedScalarFrame;
use crate::bytes::lp_ascii_filtered_view;
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::design::design_feature_family;
use crate::design::DesignFeatureFamily;
use crate::ids::native_stream;
use crate::layout::current_extrude_operation_fields as extrude_fields;
use crate::records::feature::extrude::DesignExtrudeExtent;
use crate::records::feature::extrude::DesignExtrudePrologue;
use crate::records::feature::fixed_parameters::DesignFixedChamferDistance;
use crate::records::feature::fixed_parameters::DesignFixedChamferParameters;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeDistance;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeParameters;
use crate::records::feature::fixed_parameters::DesignFixedExtrudeScalar;
use crate::records::feature::fixed_parameters::DesignFixedFilletGroup;
use crate::records::feature::fixed_parameters::DesignFixedFilletParameters;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::parameters::DesignParameter;
use crate::records::parameters::DesignParameterOwner;
use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::{NonZeroReal, PositiveReal};

/// Fixed scalar lanes of a distance Extrude. A one-sided or symmetric Extrude
/// uses lane zero for the first-side distance and lane one for its taper. A
/// two-sided Extrude adds lane two for the second-side distance and lane three
/// for the second-side taper. A one-sided reference-aware Extrude whose lanes
/// do not follow that order names each lane through its prologue.
pub(super) fn exact_fixed_extrude_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[crate::records::parameters::DesignParameterOwner],
) -> Option<DesignFixedExtrudeParameters> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Extrude) {
        return None;
    }
    exact_ordered_fixed_extrude_lanes(bytes, records, scope, parameters, parameter_owners).or_else(
        || {
            let owners = reference_aware_lane_owners(bytes, scope.extrude_prologue()?)?;
            exact_named_fixed_extrude_lanes(
                bytes,
                records,
                scope,
                &owners,
                (parameters, parameter_owners),
            )
        },
    )
}

fn exact_ordered_fixed_extrude_lanes(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[crate::records::parameters::DesignParameterOwner],
) -> Option<DesignFixedExtrudeParameters> {
    let lane_count = match scope
        .extrude_prologue()
        .and_then(DesignExtrudePrologue::extent)?
    {
        DesignExtrudeExtent::OneSidedDistance | DesignExtrudeExtent::SymmetricDistance => 2,
        DesignExtrudeExtent::TwoSidedDistance => 4,
        _ => return None,
    };
    let mut fixed_lanes = [None; 4];
    let mut fixed_count = 0;
    let mut embedded_distance = None;
    for record_index in scope.reference_members().values() {
        if let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index)
            .filter(|scalar| scalar.owner_record_index == Some(scope.record_index))
        {
            let slot = fixed_lanes[..lane_count].get_mut(fixed_count)?;
            *slot = Some((*record_index, scalar));
            fixed_count += 1;
        }
        if let Some(scalar) =
            exact_embedded_extrude_distance(bytes, records, *record_index, scope.record_index)
        {
            if embedded_distance.replace((*record_index, scalar)).is_some() {
                return None;
            }
        }
    }
    let mut along_distance = embedded_distance.map(|(record_index, lane)| {
        DesignFixedExtrudeDistance::DistanceConstruction(DesignFixedExtrudeScalar {
            value: lane.value,
            record_index,
            value_offset: lane.value_offset,
        })
    });
    let mut taper_angle = None;
    let mut against_distance = None;
    let mut side_two_taper_angle = None;
    let mut seen_fixed_ordinals = [false; 4];
    let ordinal_zero = fixed_lanes
        .iter()
        .flatten()
        .find(|(_, lane)| lane.ordinal == 0);
    let ordinal_one = fixed_lanes
        .iter()
        .flatten()
        .find(|(_, lane)| lane.ordinal == 1);
    let swapped_two_lane = lane_count == 2
        && embedded_distance.is_none()
        && ordinal_zero.is_some_and(|(_, lane)| lane.value.get() == 0.0)
        && ordinal_one.is_some_and(|(_, lane)| lane.value.get() != 0.0);
    for (record_index, lane) in fixed_lanes.into_iter().flatten() {
        let ordinal = usize::from(lane.ordinal);
        if ordinal >= lane_count || seen_fixed_ordinals[ordinal] {
            return None;
        }
        seen_fixed_ordinals[ordinal] = true;
        let source_kind = parameter_owners
            .iter()
            .find(|owner| {
                native_stream(owner.id()) == native_stream(&scope.id)
                    && owner.scope_record_index() == scope.record_index
                    && owner.record_index() == record_index
            })
            .and_then(|owner| {
                parameters
                    .iter()
                    .find(|parameter| {
                        native_stream(&parameter.id) == native_stream(&scope.id)
                            && parameter.record_index == owner.parameter_record_index()
                    })
                    .map(crate::records::parameters::DesignParameter::source_kind)
            });
        let scalar = DesignFixedExtrudeScalar {
            value: lane.value,
            record_index,
            value_offset: lane.value_offset,
        };
        let nonzero = |scalar: DesignFixedExtrudeScalar| {
            Some(DesignFixedExtrudeScalar {
                value: NonZeroReal::new(scalar.value.get())?,
                record_index: scalar.record_index,
                value_offset: scalar.value_offset,
            })
        };
        let lane_kind = match source_kind {
            Some(kind) => kind.as_str(),
            None if swapped_two_lane => match lane.ordinal {
                0 => "TaperAngle",
                _ => "AlongDistance",
            },
            None => match lane.ordinal {
                0 => "AlongDistance",
                1 => "TaperAngle",
                2 => "AgainstDistance",
                _ => "Side2TaperAngle",
            },
        };
        match lane_kind {
            "AlongDistance" if along_distance.is_none() && lane.value.get() != 0.0 => {
                along_distance = Some(DesignFixedExtrudeDistance::FixedScalar(nonzero(scalar)?));
            }
            "AlongDistance" if along_distance.is_some() && lane.value.get() == 0.0 => {}
            "TaperAngle" if taper_angle.is_none() => taper_angle = Some(scalar),
            "AgainstDistance" if lane_count == 4 && against_distance.is_none() => {
                against_distance = Some(nonzero(scalar)?);
            }
            "Side2TaperAngle" if lane_count == 4 && side_two_taper_angle.is_none() => {
                side_two_taper_angle = Some(scalar);
            }
            _ => return None,
        }
    }
    if along_distance.is_none() && taper_angle.is_none() {
        return None;
    }
    if lane_count == 4 && (along_distance.is_none() || against_distance.is_none()) {
        return None;
    }
    Some(DesignFixedExtrudeParameters {
        along_distance,
        taper_angle,
        against_distance,
        side_two_taper_angle,
        profile_offset: None,
        side_one_offset: None,
        side_two_offset: None,
    })
}

/// Lane owners that a one-sided reference-aware Extrude prologue names.
struct ReferenceAwareLaneOwners {
    /// Nullable slot zero: the start offset.
    profile_offset: Option<u32>,
    /// Nullable slot two: the first-side distance.
    along_distance: Option<u32>,
    /// Nullable slot five: the first-side taper.
    taper_angle: Option<u32>,
    /// First reference of a first-side extent `2` target payload: the
    /// termination offset.
    side_one_offset: Option<u32>,
    side_two_offset: Option<u32>,
    against_distance: Option<u32>,
    side_two_taper_angle: Option<u32>,
}

/// Read the seven nullable reference slots after the profile normal of a
/// one-sided distance or to-face reference-aware Extrude.
///
/// The slot run must end at the stored first-side extent value, which
/// excludes the shape-target form.
fn reference_aware_lane_owners(
    bytes: &[u8],
    prologue: DesignExtrudePrologue,
) -> Option<ReferenceAwareLaneOwners> {
    let DesignExtrudePrologue::ReferenceAware {
        operation_offset,
        extent,
        side_extent_discriminators,
        side_extent_discriminator_offsets,
        first_side_target_ordinal: None,
        ..
    } = prologue
    else {
        return None;
    };
    if !matches!(
        extent,
        DesignExtrudeExtent::OneSidedDistance
            | DesignExtrudeExtent::OneSidedToFace
            | DesignExtrudeExtent::TwoSidedDistance
            | DesignExtrudeExtent::TwoSidedToFaces
    ) {
        return None;
    }
    let mut at = usize::try_from(operation_offset)
        .ok()?
        .checked_add(extrude_fields::PROFILE_NORMAL + 3 * 8)?;
    let mut slots = [None; 7];
    for slot in &mut slots {
        match bytes.get(at)? {
            0 => at += 1,
            1 => {
                *slot = Some(marked_record_reference(bytes, at)?);
                at += 11;
            }
            _ => return None,
        }
    }
    if u64::try_from(at).ok()? != side_extent_discriminator_offsets[0] {
        return None;
    }
    let side_one_offset = match side_extent_discriminators[0] {
        2 => Some(marked_record_reference(bytes, at.checked_add(4)?)?),
        _ => None,
    };
    let (side_two_offset, side_two_taper_angle) = if extent == DesignExtrudeExtent::TwoSidedToFaces
    {
        let second =
            at.checked_add(crate::layout::current_extrude_face_target_payload::SECOND_SIDE_EXTENT)?;
        if View::u32_le_at(bytes, second)? != 2 {
            return None;
        }
        (Some(marked_record_reference(bytes, at.checked_add(crate::layout::current_extrude_face_target_payload::SECOND_SIDE_OFFSET_REFERENCE)?)?),
            Some(marked_record_reference(bytes, at.checked_add(crate::layout::current_extrude_face_target_payload::SECOND_SIDE_TAPER_REFERENCE)?)?))
    } else if extent == DesignExtrudeExtent::TwoSidedDistance {
        (
            None,
            marked_record_reference(
                bytes,
                usize::try_from(side_extent_discriminator_offsets[1])
                    .ok()?
                    .checked_add(4)?,
            ),
        )
    } else {
        (None, None)
    };
    Some(ReferenceAwareLaneOwners {
        profile_offset: slots[0],
        along_distance: slots[2],
        taper_angle: slots[5],
        side_one_offset,
        side_two_offset,
        against_distance: (extent == DesignExtrudeExtent::TwoSidedDistance)
            .then_some(slots[1])
            .flatten(),
        side_two_taper_angle: side_two_taper_angle.or(slots[4]),
    })
}

/// Assign every fixed scalar lane owned by the scope to the prologue slot
/// that names it. A lane that no slot names, or whose linked parameter has
/// another source kind, leaves the lanes unassigned.
fn exact_named_fixed_extrude_lanes(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    owners: &ReferenceAwareLaneOwners,
    (parameters, parameter_owners): (&[DesignParameter], &[DesignParameterOwner]),
) -> Option<DesignFixedExtrudeParameters> {
    let linked_source_kind = |record_index: u32| {
        parameter_owners
            .iter()
            .find(|owner| {
                native_stream(owner.id()) == native_stream(&scope.id)
                    && owner.scope_record_index() == scope.record_index
                    && owner.record_index() == record_index
            })
            .and_then(|owner| {
                parameters
                    .iter()
                    .find(|parameter| {
                        native_stream(&parameter.id) == native_stream(&scope.id)
                            && parameter.record_index == owner.parameter_record_index()
                    })
                    .map(DesignParameter::source_kind)
            })
    };
    let mut parameters = DesignFixedExtrudeParameters {
        along_distance: None,
        taper_angle: None,
        against_distance: None,
        side_two_taper_angle: None,
        profile_offset: None,
        side_one_offset: None,
        side_two_offset: None,
    };
    let mut lane_count = 0;
    for record_index in scope.reference_members().values() {
        let Some(lane) = exact_fixed_scalar(bytes, records, *record_index)
            .filter(|scalar| scalar.owner_record_index == Some(scope.record_index))
        else {
            continue;
        };
        lane_count += 1;
        let scalar = DesignFixedExtrudeScalar {
            value: lane.value,
            record_index: *record_index,
            value_offset: lane.value_offset,
        };
        let expected_kind = if owners.along_distance == Some(*record_index) {
            "AlongDistance"
        } else if owners.taper_angle == Some(*record_index) {
            "TaperAngle"
        } else if owners.profile_offset == Some(*record_index) {
            "ProfileOffset"
        } else if owners.side_one_offset == Some(*record_index) {
            "Side1Offset"
        } else if owners.side_two_offset == Some(*record_index) {
            "Side2Offset"
        } else if owners.against_distance == Some(*record_index) {
            "AgainstDistance"
        } else if owners.side_two_taper_angle == Some(*record_index) {
            "Side2TaperAngle"
        } else {
            return None;
        };
        if linked_source_kind(*record_index).is_some_and(|kind| kind.as_str() != expected_kind) {
            return None;
        }
        if owners.against_distance == Some(*record_index) {
            let scalar = DesignFixedExtrudeScalar {
                value: NonZeroReal::new(scalar.value.get())?,
                record_index: scalar.record_index,
                value_offset: scalar.value_offset,
            };
            if parameters.against_distance.replace(scalar).is_some() {
                return None;
            }
            continue;
        }
        let slot = if owners.along_distance == Some(*record_index) {
            &mut parameters.along_distance
        } else {
            let slot = if owners.taper_angle == Some(*record_index) {
                &mut parameters.taper_angle
            } else if owners.profile_offset == Some(*record_index) {
                &mut parameters.profile_offset
            } else if owners.side_one_offset == Some(*record_index) {
                &mut parameters.side_one_offset
            } else if owners.side_two_offset == Some(*record_index) {
                &mut parameters.side_two_offset
            } else if owners.side_two_taper_angle == Some(*record_index) {
                &mut parameters.side_two_taper_angle
            } else {
                return None;
            };
            if slot.replace(scalar).is_some() {
                return None;
            }
            continue;
        };
        let distance = DesignFixedExtrudeScalar {
            value: NonZeroReal::new(scalar.value.get())?,
            record_index: scalar.record_index,
            value_offset: scalar.value_offset,
        };
        if slot
            .replace(DesignFixedExtrudeDistance::FixedScalar(distance))
            .is_some()
        {
            return None;
        }
    }
    (lane_count > 0).then_some(parameters)
}

fn exact_embedded_extrude_distance(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    record_index: u32,
    scope_record_index: u32,
) -> Option<FixedScalarFrame<PositiveReal>> {
    let mut candidates = records.frames(record_index).filter_map(|(start, end)| {
        (end.checked_sub(start)? == 100).then_some(())?;
        let (_, after_tag) = lp_ascii_filtered_view(bytes, start, 3..=3, u8::is_ascii_digit)?;
        let first_auxiliary = record_index.checked_add(1)?;
        let second_auxiliary = record_index.checked_add(2)?;
        if after_tag != start + 7
            || bytes.get(start + 11..start + 21) != Some(&[0; 10])
            || marked_record_reference(bytes, start + 21)? != scope_record_index
            || bytes.get(start + 26..start + 32) != Some(&[0; 6])
            || View::u32_le_at(bytes, start + 32)? != 1
            || marked_record_reference(bytes, start + 36).is_none()
            || bytes.get(start + 41..start + 47) != Some(&[0; 6])
            || View::u32_le_at(bytes, start + 47)? != 210
            || View::u32_le_at(bytes, start + 59)? != 210
            || marked_record_reference(bytes, start + 63)? != second_auxiliary
            || bytes.get(start + 68..start + 74) != Some(&[0; 6])
            || bytes.get(start + 74..start + 77) != Some(&[1, 0, 0])
            || marked_record_reference(bytes, start + 77)? != first_auxiliary
            || bytes.get(start + 82..start + 89) != Some(&[0; 7])
            || marked_record_reference(bytes, start + 89)? != scope_record_index
            || bytes.get(start + 94..start + 100) != Some(&[0; 6])
        {
            return None;
        }
        let value = PositiveReal::new(View::f64_le_at(bytes, start + 51)?)?;
        Some(FixedScalarFrame {
            start: u64::try_from(start).ok()?,
            end: u64::try_from(end).ok()?,
            owner_record_index: Some(scope_record_index),
            ordinal: 0,
            value,
            value_offset: u64::try_from(start + 51).ok()?,
        })
    });
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

pub(super) fn exact_fixed_fillet_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameters: &[DesignParameter],
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignFixedFilletParameters> {
    use crate::records::feature::fixed_parameters::{
        DesignFixedFilletIntermediate, DesignFixedFilletLaw, DesignFixedFilletScalar,
    };
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Fillet) {
        return None;
    }
    let mut lanes = Vec::new();
    for record_index in scope.reference_members().values() {
        if let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index)
            .filter(|scalar| scalar.owner_record_index == Some(scope.record_index))
        {
            lanes.push((*record_index, scalar));
        }
    }
    if lanes.is_empty()
        || lanes
            .iter()
            .enumerate()
            .any(|(ordinal, (_, scalar))| usize::from(scalar.ordinal) != ordinal)
    {
        return None;
    }

    let scalar = |(record_index, scalar): &(u32, FixedScalarFrame)| DesignFixedFilletScalar {
        value: scalar.value,
        record_index: *record_index,
        value_offset: scalar.value_offset,
    };
    let group = |tangency_lane: Option<&(u32, FixedScalarFrame)>, law: DesignFixedFilletLaw| {
        DesignFixedFilletGroup::try_new(tangency_lane.map(scalar), law).ok()
    };
    let mut groups = Vec::new();
    if lanes.len() == 1 {
        let value = group(None, DesignFixedFilletLaw::Constant(scalar(&lanes[0])))?;
        groups.push(value);
    } else if lanes.len() % 2 == 0 {
        let scope_stream = native_stream(&scope.id);
        let lane_kind = |lane: u32| -> Option<Option<&str>> {
            let mut matching_owners = parameter_owners.iter().filter(|owner| {
                (scope_stream.is_none() || native_stream(owner.id()) == scope_stream)
                    && owner.scope_record_index() == scope.record_index
                    && owner.record_index() == lane
            });
            let owner = matching_owners.next();
            if matching_owners.next().is_some() {
                return None;
            }
            let Some(owner) = owner else {
                return Some(None);
            };
            let mut matching_parameters = parameters.iter().filter(|parameter| {
                (scope_stream.is_none() || native_stream(&parameter.id) == scope_stream)
                    && parameter.record_index == owner.parameter_record_index()
                    && parameter.owner_record_index() == Some(owner.record_index())
            });
            let parameter = matching_parameters.next()?;
            if matching_parameters.next().is_some() {
                return None;
            }
            Some(Some(parameter.source_kind_str()))
        };
        for pair in lanes.chunks_exact(2) {
            let first_kind = lane_kind(pair[0].0)?;
            let second_kind = lane_kind(pair[1].0)?;
            let (tangency_lane, radius_lane) = match (first_kind, second_kind) {
                (None, None) | (Some("TangencyWeight"), Some("Radius")) => (&pair[0], &pair[1]),
                (Some("Radius"), Some("TangencyWeight")) => (&pair[1], &pair[0]),
                _ => return None,
            };
            let value = group(
                Some(tangency_lane),
                DesignFixedFilletLaw::Constant(scalar(radius_lane)),
            )?;
            groups.push(value);
        }
    } else {
        let mut intermediate = Vec::new();
        for pair in lanes[3..].chunks_exact(2) {
            intermediate.push(DesignFixedFilletIntermediate {
                radius: scalar(&pair[0]),
                parameter: scalar(&pair[1]),
            });
        }
        let value = group(
            Some(&lanes[0]),
            DesignFixedFilletLaw::Variable {
                start: scalar(&lanes[1]),
                end: scalar(&lanes[2]),
                intermediate,
            },
        )?;
        groups.push(value);
    }
    Some(DesignFixedFilletParameters { groups })
}

pub(super) fn exact_fixed_chamfer_parameters(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    scope: &DesignParameterScope,
    parameter_owners: &[DesignParameterOwner],
) -> Option<DesignFixedChamferParameters> {
    if design_feature_family(&scope.kind()) != Some(DesignFeatureFamily::Chamfer) {
        return None;
    }
    let stream = native_stream(&scope.id);
    if parameter_owners.iter().any(|owner| {
        stream.is_some()
            && native_stream(owner.id()) == stream
            && owner.scope_record_index() == scope.record_index
    }) {
        return None;
    }
    let mut lanes = [None; 2];
    let mut lane_count = 0;
    for record_index in scope.reference_members().values() {
        if let Some(scalar) = exact_fixed_scalar(bytes, records, *record_index)
            .filter(|scalar| scalar.owner_record_index == Some(scope.record_index))
        {
            if usize::from(scalar.ordinal) != lane_count {
                return None;
            }
            let slot = lanes.get_mut(lane_count)?;
            *slot = Some((*record_index, scalar));
            lane_count += 1;
        }
    }
    let mut distances = lanes.into_iter().flatten().map(|(record_index, scalar)| {
        Some(DesignFixedChamferDistance {
            value: cadmpeg_ir::scalar::PositiveReal::new(scalar.value.get())?,
            record_index,
            value_offset: scalar.value_offset,
        })
    });
    let first = distances.next()??;
    Some(match distances.next() {
        Some(Some(second)) => DesignFixedChamferParameters::TwoDistances { first, second },
        None => DesignFixedChamferParameters::EqualDistance { distance: first },
        Some(None) => return None,
    })
}

/// The one positive angle owner of a `Revolve` scope at `local_ordinal`: zero
/// for travel in the revolution direction, one for the opposite side.
pub(super) fn unique_revolve_angle_owner<'a>(
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
    record_index: Option<u32>,
    local_ordinal: u32,
) -> Option<&'a DesignParameterOwner> {
    revolve_angle_owner(
        scope,
        parameter_owners,
        record_index,
        local_ordinal,
        |angle| angle > 0.0,
    )
}

/// The angle owner at local ordinal zero that a referenced-angle `Revolve`
/// scope names; its angle can be negative.
pub(super) fn referenced_revolve_angle_owner<'a>(
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
    record_index: u32,
) -> Option<&'a DesignParameterOwner> {
    revolve_angle_owner(scope, parameter_owners, Some(record_index), 0, |_| true)
}

/// The zero-valued owner at local ordinal zero that a to-object `Revolve`
/// scope names.
pub(super) fn to_object_revolve_owner<'a>(
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
    record_index: u32,
) -> Option<&'a DesignParameterOwner> {
    revolve_angle_owner(scope, parameter_owners, Some(record_index), 0, |value| {
        value == 0.0
    })
}

fn revolve_angle_owner<'a>(
    scope: &DesignParameterScope,
    parameter_owners: &'a [DesignParameterOwner],
    record_index: Option<u32>,
    local_ordinal: u32,
    admits: impl Fn(f64) -> bool,
) -> Option<&'a DesignParameterOwner> {
    let mut candidates = parameter_owners.iter().filter(|owner| {
        native_stream(owner.id()) == native_stream(&scope.id)
            && owner.scope_record_index() == scope.record_index
            && scope
                .reference_members()
                .values()
                .any(|value| value == &owner.record_index())
            && record_index.is_none_or(|index| owner.record_index() == index)
            && owner.local_ordinal() == local_ordinal
            && admits(owner.evaluated_value().get())
    });
    let angle = candidates.next()?;
    candidates.next().is_none().then_some(angle)
}
