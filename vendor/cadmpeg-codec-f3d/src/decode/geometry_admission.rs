// SPDX-License-Identifier: Apache-2.0
//! Admit vertex tolerances and cache-first spring support relations.

mod vertex_tolerance;

use crate::loss::F3dLossCode;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    curve_payloads::SpringCurvePayload, ProceduralCurveDefinition, SpringLayout,
};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::HashMap;

pub(super) fn admit(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    annotations: &mut [cadmpeg_asm::brep::annotations::AnnotationRecord],
) -> Result<(), CodecError> {
    vertex_tolerance::admit(ctx, ir, losses, annotations)?;
    for curve in &ir.model.procedural_curves {
        let context = match curve.definition() {
            ProceduralCurveDefinition::Intersection { context, .. } => context,
            ProceduralCurveDefinition::SurfaceCurve { family } => family.context(),
            ProceduralCurveDefinition::SurfaceOffset(offset) => offset.context(),
            ProceduralCurveDefinition::Deformable(deformable) => deformable.context(),
            ProceduralCurveDefinition::Spring(spring) => spring.support_context(),
            _ => continue,
        };
        for (side, support) in context.sides().iter().enumerate() {
            if support.surface.is_none() && support.pcurve.is_some() {
                losses.push(F3dLossCode::ProceduralSupportChartUnresolved.note(format!(
                    "Curve {} support side {side} retains its native support fields without a neutral surface chart",
                    curve.id
                )));
            }
        }
    }

    if !ir.model.procedural_curves.iter().any(|curve| {
        matches!(curve.definition(), ProceduralCurveDefinition::Spring(spring)
            if matches!(spring.layout(), SpringLayout::CacheFirst { .. }))
    }) {
        return Ok(());
    }
    let index = ModelIndex::new_model_only(ir);
    let mut owners = HashMap::new();
    for curve in &ir.model.curves {
        if let Some(construction) = curve.geometry.procedural_construction() {
            owners
                .entry(construction)
                .and_modify(|owner| *owner = None)
                .or_insert(Some(curve));
        }
    }
    let mut replacements = Vec::new();
    for (slot, curve) in ir.model.procedural_curves.iter().enumerate() {
        let ProceduralCurveDefinition::Spring(spring) = curve.definition() else {
            continue;
        };
        if !matches!(spring.layout(), SpringLayout::CacheFirst { .. }) {
            continue;
        }
        let Some(owner) = owners.get(&curve.id).copied().flatten() else {
            continue;
        };
        let sides = cadmpeg_ir::validate::geometry_consistency::cache_support_endpoint_failures(
            ctx,
            &index,
            owner,
            spring.support_context(),
            curve
                .cache_fit_tolerance()
                .map(cadmpeg_ir::geometry::FitTolerance::get),
        )?;
        if sides.is_empty() {
            continue;
        }
        let mut layout = spring.layout().to_raw();
        let SpringLayout::CacheFirst { context, .. } = &mut layout else {
            continue;
        };
        for side in sides {
            context.set_unmapped_pcurve(side, None);
        }
        let replacement = SpringCurvePayload::try_new(layout, *spring.direction())
            .map_err(CodecError::malformed)?;
        replacements.push((slot, replacement));
    }
    for (slot, replacement) in replacements {
        let curve = &mut ir.model.procedural_curves[slot];
        curve.replace_definition(ProceduralCurveDefinition::Spring(replacement));
        losses.push(F3dLossCode::SpringSupportPcurveUnresolved.note(format!(
            "Curve {} retains native support pcurves whose endpoint relation is unresolved",
            curve.id
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
