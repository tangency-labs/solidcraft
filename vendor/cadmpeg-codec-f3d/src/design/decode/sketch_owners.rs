// SPDX-License-Identifier: Apache-2.0
//! Bind sketch collection members to component definitions.

use std::collections::BTreeMap;

use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::{DecodeContext, ParseError, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::products::ProductDefinition;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::sketches::SketchId;

use crate::bytes::take_reference;
use crate::container::ContainerScan;
use crate::layout::sketch_ordinal_collection_prefix as prefix;
use crate::layout::sketch_owner_collection_legacy_list_prefix as legacy;
use crate::layout::sketch_owner_reference as owner_ref;
use crate::layout::sketch_owner_typed_reference as typed_owner_ref;
use crate::loss::F3dLossCode;
use crate::native::F3dNative;

const COLLECTION_GUID: &str = "5275CBA4-3D7D-40DC-A651-1DFF3FC8AFBF";

/// A collection frame that does not decode, or a decode-budget failure.
#[derive(Debug)]
enum OwnerError {
    Framing,
    Codec(CodecError),
}

impl From<ParseError> for OwnerError {
    fn from(_: ParseError) -> Self {
        Self::Framing
    }
}

impl From<CodecError> for OwnerError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

fn invalid(_: View<'_>) -> OwnerError {
    OwnerError::Framing
}

fn lp_bytes<'a>(view: &mut View<'a>) -> Result<&'a [u8], OwnerError> {
    let length = view.req_u32_le()?;
    let count = view
        .counted(u64::from(length), 1)
        .ok_or_else(|| invalid(*view))?;
    Ok(view.req_take(count.get())?)
}

fn reference(view: &mut View<'_>) -> Result<u64, OwnerError> {
    let mut end = 0;
    let parsed = take_reference(view.unread(), &mut end).ok_or_else(|| invalid(*view))?;
    let (target, _) = parsed.local().ok_or_else(|| invalid(*view))?;
    view.req_take(end)?;
    Ok(target)
}

struct Collection {
    members: Vec<u64>,
    owner: Option<u64>,
}

fn parse_collection(
    ctx: &DecodeContext<'_>,
    mut view: View<'_>,
    version: u32,
) -> Result<Collection, OwnerError> {
    let whole = view;
    view.req_take(prefix::ZERO_RUN)?;
    if view
        .req_take(prefix::LEN - prefix::ZERO_RUN)?
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(invalid(view));
    }
    let mut has_counter = false;
    match view.req_u8()? {
        0 => {}
        1 => {
            let declared = view.req_u32_le()?;
            let count = view
                .counted(u64::from(declared), 16)
                .ok_or_else(|| invalid(view))?;
            for _ in 0..count.get() {
                let name = lp_bytes(&mut view)?;
                if lp_bytes(&mut view)? != b"IntrinsicMetaTypeuint64" {
                    return Err(invalid(view));
                }
                view.req_u64_le()?;
                has_counter |= name == b"crv_tag_id_idx";
            }
        }
        _ => return Err(invalid(view)),
    }
    if !has_counter {
        return Err(invalid(view));
    }
    if version == 3 {
        view.req_take(legacy::LEN)?;
    }
    let declared = view.req_u32_le()?;
    let count = view
        .counted(u64::from(declared), owner_ref::LEN)
        .ok_or_else(|| invalid(view))?;
    let mut members = ctx.alloc_filled(count.get(), 0, "F3D sketch collection members")?;
    for member in &mut members {
        let mut end = 0;
        let parsed = take_reference(view.unread(), &mut end).ok_or_else(|| invalid(view))?;
        let Some((target, None)) = parsed.local() else {
            return Err(invalid(view));
        };
        if target == 0 {
            return Err(invalid(view));
        }
        view.req_take(end)?;
        *member = target;
    }
    let owner = [typed_owner_ref::LEN, owner_ref::LEN]
        .into_iter()
        .find_map(|length| {
            let start = whole.end().checked_sub(length)?;
            if start < view.position() {
                return None;
            }
            let mut tail = whole.child(start, whole.end())?;
            let target = reference(&mut tail).ok()?;
            (tail.remaining() == 0).then_some(target)
        });
    Ok(Collection { members, owner })
}

/// Attach projected planar sketches through their collections' component links.
pub(crate) fn attach(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    native: &F3dNative,
    model: &mut cadmpeg_ir::document::Model,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if model.sketches.is_empty() {
        return Ok(());
    }
    let components =
        super::components::decode_local_components(scan, &native.design_component_naming_spaces)?;
    let implicit_roots = components
        .values()
        .filter_map(|stream| {
            stream
                .root_component
                .and_then(|root| stream.roots.get(&root))
                .or_else(|| {
                    if stream.roots.len() == 1 {
                        stream.roots.values().next()
                    } else {
                        None
                    }
                })
        })
        .map(|root| crate::ids::neutral_component_id(&root.context_uuid))
        .collect();
    let mut owners = BTreeMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let stream = crate::ids::native_scope(&entry.name);
        let Some(meta) = super::meta::metadata_for_bulk_stream(scan, &entry.name)? else {
            continue;
        };
        let view = scan
            .entry_view(&entry.name)
            .ok_or_else(|| CodecError::malformed("missing Design stream"))?;
        let frames = super::meta::typed_primary_frames(
            view.window(),
            &meta,
            COLLECTION_GUID,
            "sketch owner collection",
        )?;
        let mut entities = BTreeMap::new();
        for frame in frames {
            if ![3, 4].contains(&frame.design_type.version)
                || frame.design_type.module != "Geometry"
                || !frame
                    .design_type
                    .base_type_guid
                    .value()
                    .is_some_and(|base| {
                        base.as_str()
                            .eq_ignore_ascii_case("A7AEA631-985B-4DD1-8CE2-DE2C-14B54081")
                    })
            {
                continue;
            }
            let frame_view = view
                .child(frame.start, frame.end)
                .ok_or_else(|| CodecError::malformed("invalid sketch collection extent"))?;
            let collection = match parse_collection(ctx, frame_view, frame.design_type.version) {
                Ok(collection) => collection,
                Err(OwnerError::Framing) => {
                    losses.push(F3dLossCode::SketchOwnerUnresolved.note(
                        "A sketch collection frame does not decode; its sketches remain at the design root.",
                    ));
                    continue;
                }
                Err(OwnerError::Codec(error)) => return Err(error),
            };
            // A collection without an owner reference holds Design-root sketches.
            let Some(owner) = collection.owner else {
                continue;
            };
            let definition = components
                .get(&stream)
                .and_then(|stream| stream.roots.get(&owner))
                .map(|owner| crate::ids::neutral_component_id(&owner.context_uuid));
            for member in collection.members {
                match entities.entry(member) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(definition.clone());
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        entry.insert(None);
                    }
                }
            }
        }
        for placement in native
            .design_sketch_placements
            .iter()
            .filter(|placement| crate::ids::native_stream(&placement.id) == Some(stream.as_str()))
        {
            let id = crate::ids::neutral_sketch_id(placement);
            if let Some(owner) = entities.get(&placement.entity_id.suffix()) {
                owners.insert(id, owner.clone());
            }
        }
    }
    assign(
        &owners,
        &model
            .sketches
            .iter()
            .map(|sketch| sketch.id.clone())
            .collect(),
        &mut model.product_definitions,
        &implicit_roots,
        losses,
    );
    Ok(())
}

fn assign(
    owners: &BTreeMap<SketchId, Option<ProductDefinitionId>>,
    sketches: &std::collections::BTreeSet<SketchId>,
    definitions: &mut [ProductDefinition],
    implicit_roots: &std::collections::BTreeSet<ProductDefinitionId>,
    losses: &mut Vec<LossNote>,
) {
    for (sketch, owner) in owners {
        if !sketches.contains(sketch) {
            continue;
        }
        match owner {
            Some(owner) => {
                let definition = definitions
                    .iter_mut()
                    .find(|definition| definition.id == *owner);
                if let Some(definition) = definition {
                    definition.sketches.push(sketch.clone());
                } else if !implicit_roots.contains(owner) {
                    losses.push(F3dLossCode::SketchOwnerUnresolved.note(
                        "A sketch collection names a component without a projected product definition; the sketch remains at the design root.",
                    ));
                }
            }
            None => {
                losses.push(F3dLossCode::SketchOwnerUnresolved.note(
                    "A sketch collection owner reference does not resolve to one component definition; the sketch remains at the design root.",
                ));
            }
        }
    }
    for definition in definitions {
        definition.sketches.sort();
        definition.sketches.dedup();
    }
}

#[cfg(test)]
mod tests;
