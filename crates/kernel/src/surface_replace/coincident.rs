//! A contained skin touching planar base walls, certified by the caller's exact
//! positive-weight control hulls. Trim each wall in its own plane, omit the
//! overlapping skin faces, then sew the remaining exact boundaries. Avoid a 3D
//! intersection between surfaces that coincide over whole regions. All history
//! is composed back to `[base, skin]`, including the 2D Boolean's edge tags.
use std::collections::{BTreeMap, BTreeSet};

use solidcore_record::brep::{self as b};
use solidcraft_geom::brep::pcurve::{EdgeUse, exact_pcurves};

use crate::history::{Change, EdgeOrigin};
use crate::surfaces::Parts;
use crate::{Body, KernelError, Result, boolean_record, record};

fn invalid(message: impl Into<String>) -> KernelError {
    KernelError::Invalid(message.into())
}

pub(super) fn cut(base: &Body, opening: usize, skin: &Body, contacts: &BTreeMap<usize, Vec<usize>>, tolerance: f64) -> Result<Option<Body>> {
    crate::guard("trim coincident Form walls", || {
        let counts = record::counts(&[base, skin]);
        let removed: BTreeSet<usize> = contacts.values().flatten().copied().collect();
        if removed.is_empty() || removed.len() == skin.face_count() {
            return Ok(None);
        }
        let mut pieces = Vec::with_capacity(base.face_count());
        for wall in 0..base.face_count() {
            if wall == opening {
                continue;
            }
            let sheet = crate::surfaces::taken(base, base.record(), &[(wall, false)])?;
            let history = boolean_record::placed(sheet.history().cloned(), 0, 2);
            let mut remaining = Some(sheet.with_checked_history(history, &counts));
            for &face in contacts.get(&wall).into_iter().flatten() {
                let Some(target) = remaining.take() else { break };
                let tool = contact_on_wall(skin, face, base, wall, tolerance)?;
                remaining = difference(&target, &tool, &counts)?;
            }
            if let Some(sheet) = remaining {
                pieces.push(sheet);
            }
        }
        let interior: Vec<_> = (0..skin.face_count()).filter(|i| !removed.contains(i)).map(|i| (i, false)).collect();
        let sheet = crate::surfaces::taken(skin, skin.record(), &interior)?;
        let history = boolean_record::placed(sheet.history().cloned(), 1, 2);
        pieces.push(sheet.with_checked_history(history, &counts));
        // The caller permits up to 256 base walls; split sewing takes 64
        // operands at once. Compose partial sewing in bounded batches rather
        // than introducing a smaller limit on supported polyhedra. The final
        // sew still checks the complete vertex/edge work budget.
        while pieces.len() > 64 {
            pieces = pieces.chunks(64).map(|batch| sewn(batch, tolerance, &counts)).collect::<Result<Vec<_>>>()?;
        }
        let result = sewn(&pieces, tolerance, &counts)?;
        if result.is_surface() || result.shell_count() != 1 || !result.validity().is_empty() {
            return Ok(None);
        }
        Ok(Some(result))
    })
}

/// The certified planar skin face on the original wall's analytic support. Keep
/// the skin's exact 3D curves, intervals and weights; its pcurves are their affine
/// projections onto that plane. This is a representation change, not a refit.
fn contact_on_wall(skin: &Body, face: usize, base: &Body, wall: usize, tolerance: f64) -> Result<Body> {
    let original = skin.record().faces.get(face).ok_or_else(|| invalid("missing coincident skin face"))?;
    let support = base.record().faces.get(wall).ok_or_else(|| invalid("missing coincident support wall"))?;
    let turn = solidcore_ops::face_surface::opposed(original, support).map_err(|_| invalid("a coincident face has no regular normal"))?;
    let mut parts = Parts::of(skin.record(), &[(face, false)])?;
    let (flat, forward) = parts.faces.first_mut().ok_or_else(|| invalid("missing coincident face parts"))?;
    if turn {
        *forward = !*forward;
        for boundary in &mut flat.loops {
            let b::Loop::Edges { edges } = boundary else { return Err(invalid("a coincident face has a vertex loop")) };
            edges.reverse();
            for use_ in edges {
                use_.forward = !use_.forward;
            }
        }
    }
    flat.surface = support.surface.clone();
    flat.nurbs = None;
    // Containment has been certified in every base half-space, so this wall's
    // complete parameter box also contains the contact's trimmed domain.
    flat.domain = support.domain;
    let loops = flat
        .loops
        .iter()
        .map(|boundary| {
            let b::Loop::Edges { edges } = boundary else { return Err(invalid("a coincident face has a vertex loop")) };
            edges
                .iter()
                .map(|use_| {
                    let edge = parts.edges.get(use_.edge as usize).ok_or_else(|| invalid("missing coincident boundary edge"))?;
                    Ok(EdgeUse {
                        edge: use_.edge,
                        curve: edge.parameters(),
                        interval: edge.interval,
                        forward: use_.forward,
                        tolerance: edge.tolerance,
                        given: None,
                    })
                })
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let pcurves = exact_pcurves(&flat.surface, &loops, tolerance, Some(flat.domain)).map_err(|e| invalid(e.to_string()))?;
    for (boundary, pcurves) in flat.loops.iter_mut().zip(pcurves) {
        let b::Loop::Edges { edges } = boundary else { return Err(invalid("a coincident face has a vertex loop")) };
        for (use_, pcurve) in edges.iter_mut().zip(pcurves) {
            use_.pcurve = pcurve.ok_or_else(|| invalid("a coincident boundary has no exact planar projection"))?;
        }
    }
    let canonical = parts.canonical()?;
    let made = Body::from_record(canonical.brep)?;
    let edges = canonical.edges.iter().map(|&edge| vec![EdgeOrigin::Edge { operand: 1, edge }]).collect::<Vec<_>>();
    let history = record::of_faces(made.record(), 2, vec![record::from_face(1, face, Change::Kept)], &edges);
    made.with_history_counts(history, &record::counts(&[base, skin]))
}

/// Two coplanar sheets differenced on their exact curves. Each input already has
/// history over `[base, skin]`; compose the native tags through both inputs.
fn difference(a: &Body, b: &Body, counts: &[(usize, usize)]) -> Result<Option<Body>> {
    let failed = |e| KernelError::smlib("Form planar wall difference", e);
    let (sa, oa) = crate::native::smlib_body_ordered(a)?;
    let (sb, ob) = crate::native::smlib_body_ordered(b)?;
    let ta = sa.tagged(0).map_err(failed)?;
    let tb = sb.tagged(1).map_err(failed)?;
    let Some(result) = ta.boolean2d(&tb, solidcraft_smlib::BooleanOp::Subtract).map_err(failed)? else { return Ok(None) };
    let check = result.check(solidcraft_smlib::CheckLevel::All).map_err(failed)?;
    if !check.is_valid() {
        return Err(KernelError::Failed(format!("Form planar wall difference: {check}")));
    }
    let (body, order) = crate::native::body_from_smlib_ordered(&result)?;
    let history = (|| {
        let (oa, ob, order) = (oa?, ob?, order?);
        let native = result.history().map_err(failed).ok()?;
        let history = boolean_record::composed(native.history(&[oa, ob], &order))?;
        boolean_record::composed(history.over(&[a.history()?, b.history()?]))
    })();
    Ok(Some(body.with_checked_history(history, counts)))
}

/// Sew a bounded batch whose members all refer to the original two operands.
fn sewn(pieces: &[Body], tolerance: f64, counts: &[(usize, usize)]) -> Result<Body> {
    let operands = pieces.iter().collect::<Vec<_>>();
    let result = crate::stitch_split(&operands, tolerance)?;
    let history = (|| {
        let histories = pieces.iter().map(Body::history).collect::<Option<Vec<_>>>()?;
        boolean_record::composed(result.history()?.clone().over(&histories))
    })();
    Ok(result.with_checked_history(history, counts))
}
