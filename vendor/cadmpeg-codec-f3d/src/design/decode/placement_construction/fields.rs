// SPDX-License-Identifier: Apache-2.0
//! Read bounded versioned placement members before their base level.

use crate::bytes::take_reference;
use crate::records::identity::Located;
use crate::records::placement_construction::{
    PlacementFields, PlacementReference, PlacementReferenceList,
};
use crate::records::sketch_placement::SketchPlacementMatrix;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::FiniteReal;

#[derive(Clone, Copy)]
pub(super) struct ReferenceListSpan<'a> {
    count: Located<u32>,
    bytes: View<'a>,
}

type BorrowedFields<'a> = PlacementFields<Located<&'a [u8]>, ReferenceListSpan<'a>>;

fn byte(view: &mut View<'_>) -> Option<Located<u8>> {
    let offset = u64_from_index(view.position());
    Some(Located {
        offset,
        value: view.u8()?,
    })
}
fn word(view: &mut View<'_>) -> Option<Located<u32>> {
    let offset = u64_from_index(view.position());
    Some(Located {
        offset,
        value: view.u32_le()?,
    })
}
fn real(view: &mut View<'_>) -> Option<Located<FiniteReal>> {
    let offset = u64_from_index(view.position());
    Some(Located {
        offset,
        value: FiniteReal::new(view.f64_le()?)?,
    })
}
fn reals<const N: usize>(view: &mut View<'_>) -> Option<Located<[FiniteReal; N]>> {
    let offset = u64_from_index(view.position());
    let mut values = [FiniteReal::ZERO; N];
    for value in &mut values {
        *value = FiniteReal::new(view.f64_le()?)?;
    }
    Some(Located {
        offset,
        value: values,
    })
}
fn reference<'a>(view: &mut View<'a>) -> Option<Located<&'a [u8]>> {
    let offset = u64_from_index(view.position());
    let mut cursor = 0;
    take_reference(view.unread(), &mut cursor)?;
    Some(Located {
        offset,
        value: view.take(cursor)?,
    })
}
fn references<'a>(view: &mut View<'a>) -> Option<ReferenceListSpan<'a>> {
    let count = word(view)?;
    let bounded = view.counted(u64::from(count.value), 1)?;
    let start = view.position();
    for _ in 0..bounded.get() {
        reference(view)?;
    }
    Some(ReferenceListSpan {
        count,
        bytes: view.child(start, view.position())?,
    })
}
fn matrix(view: &mut View<'_>) -> Option<Located<SketchPlacementMatrix>> {
    let offset = u64_from_index(view.position());
    let mut rows = [[0.0; 4]; 4];
    for value in rows.iter_mut().flatten() {
        *value = view.f64_le()?;
    }
    Some(Located {
        offset,
        value: SketchPlacementMatrix::try_from(rows).ok()?,
    })
}

pub(super) fn scan(
    mut view: View<'_>,
    version: u32,
    marked: bool,
) -> Option<(BorrowedFields<'_>, View<'_>)> {
    let linear_offset_on_periodic_face = if version >= 14 {
        Some(byte(&mut view)?)
    } else {
        None
    };
    let align_up_direction = if version >= 4 {
        Some(reals(&mut view)?)
    } else {
        None
    };
    if align_up_direction.as_ref().is_some_and(|value| {
        !crate::records::placement_construction::valid_align_up_direction(&value.value)
    }) {
        return None;
    }
    let arc_length_for_periodic_face = if (11..16).contains(&version) {
        Some(real(&mut view)?)
    } else {
        None
    };
    let periodic_face_mode = if version >= 16 {
        Some(byte(&mut view)?)
    } else {
        None
    };
    let axis_flipped = if version >= 8 {
        Some(byte(&mut view)?)
    } else {
        None
    };
    let use_u_direction = if version >= 10 {
        Some(byte(&mut view)?)
    } else {
        None
    };
    let definer_creator = if version >= 6 {
        Some(reference(&mut view)?)
    } else {
        None
    };
    let extra_offset = if version >= 13 {
        Some(reference(&mut view)?)
    } else {
        None
    };
    let global_parameters = if version >= 9 {
        Some([
            reference(&mut view)?,
            reference(&mut view)?,
            reference(&mut view)?,
            reference(&mut view)?,
        ])
    } else {
        None
    };
    let inferred_x_axis = byte(&mut view)?;
    let inferred_z_axis = byte(&mut view)?;
    let z_flip = byte(&mut view)?;
    let identity = if marked {
        let raw_identity = byte(&mut view)?;
        Some(Located {
            offset: raw_identity.offset,
            value: match raw_identity.value {
                0 => false,
                1 => true,
                _ => return None,
            },
        })
    } else {
        (version == 4).then_some(())?;
        None
    };
    let matrix = if identity.as_ref().is_some_and(|identity| identity.value) {
        None
    } else {
        Some(matrix(&mut view)?)
    };
    let old_rotation = if version >= 7 {
        Some(real(&mut view)?)
    } else {
        None
    };
    let pick_point_parameter = if version == 2 {
        Some(real(&mut view)?)
    } else {
        None
    };
    let pick_point = if version >= 1 {
        Some(reals(&mut view)?)
    } else {
        None
    };
    let edge_references = if version >= 5 {
        Some(references(&mut view)?)
    } else {
        None
    };
    let origin = reference(&mut view)?;
    let plane_references = references(&mut view)?;
    let x_axis = reference(&mut view)?;
    let z_axis = reference(&mut view)?;
    let ref_type = word(&mut view)?;
    let resize_type = if version >= 12 {
        Some(byte(&mut view)?)
    } else {
        None
    };
    let values = references(&mut view)?;
    Some((
        PlacementFields {
            linear_offset_on_periodic_face,
            align_up_direction,
            arc_length_for_periodic_face,
            periodic_face_mode,
            axis_flipped,
            use_u_direction,
            definer_creator,
            extra_offset,
            global_parameters,
            inferred_x_axis,
            inferred_z_axis,
            z_flip,
            identity,
            matrix,
            old_rotation,
            pick_point_parameter,
            pick_point,
            edge_references,
            origin,
            plane_references,
            x_axis,
            z_axis,
            ref_type,
            resize_type,
            values,
        },
        view,
    ))
}

fn retain_reference(
    ctx: &DecodeContext<'_>,
    raw: Located<&[u8]>,
) -> Result<PlacementReference, CodecError> {
    let mut bytes = ctx.alloc_filled(raw.value.len(), 0u8, "placement reference encoding")?;
    bytes.copy_from_slice(raw.value);
    PlacementReference::new(raw.offset, bytes).map_err(CodecError::Malformed)
}
fn retain_optional(
    ctx: &DecodeContext<'_>,
    raw: Option<Located<&[u8]>>,
) -> Result<Option<PlacementReference>, CodecError> {
    raw.map(|raw| retain_reference(ctx, raw)).transpose()
}
fn retain_list(
    ctx: &DecodeContext<'_>,
    raw: ReferenceListSpan<'_>,
) -> Result<PlacementReferenceList, CodecError> {
    let bounded = raw
        .bytes
        .counted(u64::from(raw.count.value), 1)
        .ok_or_else(|| CodecError::malformed("placement reference list exceeds its source"))?;
    let mut members = ctx.alloc_filled(
        bounded.get(),
        PlacementReference::null(),
        "placement references",
    )?;
    let mut view = raw.bytes;
    for member in &mut members {
        let raw = reference(&mut view)
            .ok_or_else(|| CodecError::malformed("placement reference is incomplete"))?;
        *member = retain_reference(ctx, raw)?;
    }
    Ok(PlacementReferenceList {
        count: raw.count,
        members,
    })
}

pub(super) fn retain(
    ctx: &DecodeContext<'_>,
    value: &BorrowedFields<'_>,
) -> Result<PlacementFields, CodecError> {
    let global_parameters = if let Some(raw) = value.global_parameters {
        let mut values = [
            PlacementReference::null(),
            PlacementReference::null(),
            PlacementReference::null(),
            PlacementReference::null(),
        ];
        for (value, raw) in values.iter_mut().zip(raw) {
            *value = retain_reference(ctx, raw)?;
        }
        Some(values)
    } else {
        None
    };
    Ok(PlacementFields {
        linear_offset_on_periodic_face: value.linear_offset_on_periodic_face,
        align_up_direction: value.align_up_direction,
        arc_length_for_periodic_face: value.arc_length_for_periodic_face,
        periodic_face_mode: value.periodic_face_mode,
        axis_flipped: value.axis_flipped,
        use_u_direction: value.use_u_direction,
        definer_creator: retain_optional(ctx, value.definer_creator)?,
        extra_offset: retain_optional(ctx, value.extra_offset)?,
        global_parameters,
        inferred_x_axis: value.inferred_x_axis,
        inferred_z_axis: value.inferred_z_axis,
        z_flip: value.z_flip,
        identity: value.identity,
        matrix: value.matrix,
        old_rotation: value.old_rotation,
        pick_point_parameter: value.pick_point_parameter,
        pick_point: value.pick_point,
        edge_references: value
            .edge_references
            .map(|raw| retain_list(ctx, raw))
            .transpose()?,
        origin: retain_reference(ctx, value.origin)?,
        plane_references: retain_list(ctx, value.plane_references)?,
        x_axis: retain_reference(ctx, value.x_axis)?,
        z_axis: retain_reference(ctx, value.z_axis)?,
        ref_type: value.ref_type,
        resize_type: value.resize_type,
        values: retain_list(ctx, value.values)?,
    })
}
