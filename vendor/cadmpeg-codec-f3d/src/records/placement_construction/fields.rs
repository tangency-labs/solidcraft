// SPDX-License-Identifier: Apache-2.0
//! Validate the physical order of placement members and version gates.

use super::{PlacementFields, PlacementReference, PlacementReferenceList};
use crate::records::identity::Located;
use cadmpeg_core::decode::u64_from_index;

fn advance(position: &mut u64, offset: u64, width: u64) -> Result<(), String> {
    if offset != *position {
        return Err("placement member offset is not contiguous".into());
    }
    *position = position
        .checked_add(width)
        .ok_or("placement member extent overflows")?;
    Ok(())
}

fn scalar<T>(position: &mut u64, value: &Located<T>, width: u64) -> Result<(), String> {
    advance(position, value.offset, width)
}

fn optional<T>(
    position: &mut u64,
    value: Option<&Located<T>>,
    present: bool,
    width: u64,
) -> Result<(), String> {
    match (value, present) {
        (Some(value), true) => scalar(position, value, width),
        (None, false) => Ok(()),
        _ => Err("placement member presence disagrees with its type version".into()),
    }
}

fn reference(position: &mut u64, value: &PlacementReference) -> Result<(), String> {
    if value.offset != *position {
        return Err("placement reference offset is not contiguous".into());
    }
    *position = value.end().ok_or("placement reference extent overflows")?;
    Ok(())
}

fn optional_reference(
    position: &mut u64,
    value: Option<&PlacementReference>,
    present: bool,
) -> Result<(), String> {
    match (value, present) {
        (Some(value), true) => reference(position, value),
        (None, false) => Ok(()),
        _ => Err("placement reference presence disagrees with its type version".into()),
    }
}

fn references(position: &mut u64, value: &PlacementReferenceList) -> Result<(), String> {
    scalar(position, &value.count, 4)?;
    if u64::from(value.count.value) != u64_from_index(value.members.len()) {
        return Err("placement reference count disagrees with its members".into());
    }
    for member in &value.members {
        reference(position, member)?;
    }
    Ok(())
}

pub(super) fn validate(
    value: &PlacementFields,
    version: u32,
    mut position: u64,
) -> Result<u64, String> {
    optional(
        &mut position,
        value.linear_offset_on_periodic_face.as_ref(),
        version >= 14,
        1,
    )?;
    optional(
        &mut position,
        value.align_up_direction.as_ref(),
        version >= 4,
        24,
    )?;
    if value
        .align_up_direction
        .as_ref()
        .is_some_and(|value| !super::valid_align_up_direction(&value.value))
    {
        return Err("placement alignment direction must be unit or zero".into());
    }
    optional(
        &mut position,
        value.arc_length_for_periodic_face.as_ref(),
        (11..16).contains(&version),
        8,
    )?;
    optional(
        &mut position,
        value.periodic_face_mode.as_ref(),
        version >= 16,
        1,
    )?;
    optional(&mut position, value.axis_flipped.as_ref(), version >= 8, 1)?;
    optional(
        &mut position,
        value.use_u_direction.as_ref(),
        version >= 10,
        1,
    )?;
    optional_reference(&mut position, value.definer_creator.as_ref(), version >= 6)?;
    optional_reference(&mut position, value.extra_offset.as_ref(), version >= 13)?;
    match (&value.global_parameters, version >= 9) {
        (Some(values), true) => {
            for value in values {
                reference(&mut position, value)?;
            }
        }
        (None, false) => {}
        _ => return Err("placement global references disagree with its type version".into()),
    }
    scalar(&mut position, &value.inferred_x_axis, 1)?;
    scalar(&mut position, &value.inferred_z_axis, 1)?;
    scalar(&mut position, &value.z_flip, 1)?;
    match value.identity.as_ref() {
        Some(identity) => scalar(&mut position, identity, 1)?,
        None if version == 4 => {}
        None => return Err("placement identity marker is required for this version".into()),
    }
    optional(
        &mut position,
        value.matrix.as_ref(),
        value
            .identity
            .as_ref()
            .is_none_or(|identity| !identity.value),
        128,
    )?;
    optional(&mut position, value.old_rotation.as_ref(), version >= 7, 8)?;
    optional(
        &mut position,
        value.pick_point_parameter.as_ref(),
        version == 2,
        8,
    )?;
    optional(&mut position, value.pick_point.as_ref(), version >= 1, 16)?;
    match (&value.edge_references, version >= 5) {
        (Some(values), true) => references(&mut position, values)?,
        (None, false) => {}
        _ => return Err("placement edge list disagrees with its type version".into()),
    }
    reference(&mut position, &value.origin)?;
    references(&mut position, &value.plane_references)?;
    reference(&mut position, &value.x_axis)?;
    reference(&mut position, &value.z_axis)?;
    scalar(&mut position, &value.ref_type, 4)?;
    optional(&mut position, value.resize_type.as_ref(), version >= 12, 1)?;
    references(&mut position, &value.values)?;
    Ok(position)
}
