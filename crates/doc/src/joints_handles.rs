//! The instantaneous motion a joint value gives a selected occurrence. Probe the same
//! placement graph and loop solver as a drive, including reversed joints and motion links.
use std::collections::BTreeSet;

use super::{Analysis, Held, MAX_LOOP_VALUES, Solution, apply_links, build, joint_values, link_chains, solve_holding_with_analysis};
use crate::{Document, Mat, ModelState, apply_point, apply_vector, mat_inverse, mat_mul};
use solidcraft_geom::Vec3;

/// A driveable value, expressed as an axis of the selected occurrence in world coordinates.
#[derive(Clone, Debug)]
pub struct MotionAxis {
    pub joint: u64,
    pub index: usize,
    pub origin: Vec3,
    /// Increasing the value moves/turns the selected occurrence along/about this unit axis.
    pub axis: Vec3,
    pub angular: bool,
    /// Selected motion (mm or radians) per native joint-value unit (mm or radians).
    pub scale: f64,
}

/// Independent values on this occurrence's anchored chain, excluding fixed, suppressed,
/// invalid and zero-range motion. Closed loops must admit the probe before an axis is shown.
/// This is a bounded query for an explicit Move operation, not a per-hover rank analysis.
pub fn motion_axes(doc: &Document, st: &ModelState, occurrence: u64) -> Vec<MotionAxis> {
    let asm = &doc.assembly;
    let values = joint_values(asm);
    let mut report = Solution::default();
    let built = build(doc, st, &values, &mut report);
    let tree = &built.tree;
    if tree.fixed.contains_key(&occurrence) || !tree.parent.contains_key(&occurrence) || !report.errors.is_empty() {
        return Vec::new();
    }
    let Some(base) = tree.world(occurrence, &values) else { return Vec::new() };
    let Some(inverse) = mat_inverse(&base) else { return Vec::new() };
    let path: BTreeSet<u64> = tree.path(occurrence).1.iter().map(|(id, _)| *id).collect();
    let linked: BTreeSet<_> = asm.links.iter().map(|l| (l.b, l.ib)).collect();
    let mut variables: Vec<_> = tree
        .path(occurrence)
        .1
        .iter()
        .filter_map(|(id, _)| tree.joints.get(id))
        .flat_map(|j| (0..j.kind.dofs().len()).map(move |i| (j.id, i)))
        .filter(|v| !linked.contains(v))
        .collect();
    variables.extend(link_chains(asm, tree, &path, &linked).1);
    let mut seen = BTreeSet::new();
    variables.retain(|v| seen.insert(*v));
    let reference = tree
        .path(occurrence)
        .1
        .first()
        .and_then(|(id, from_a)| {
            let (j, frames) = (tree.joints.get(id)?, tree.frames.get(id)?);
            let (owner, frame) = if *from_a { (j.b.occurrence, &frames.b) } else { (j.a.occurrence, &frames.a) };
            Some(apply_point(&mat_mul(&tree.world(owner, &values)?, frame), Vec3::ZERO))
        })
        .unwrap_or_else(|| apply_point(&base, Vec3::ZERO));
    let mut out: Vec<MotionAxis> = Vec::new();
    for (id, index) in variables.into_iter().take(MAX_LOOP_VALUES) {
        let Some(joint) = tree.joints.get(&id) else { continue };
        // A parameter expression constrains this value; moving it must not silently erase it.
        if joint.exprs.value(index).is_some() {
            continue;
        }
        let Some(q) = values.get(&id).and_then(|q| q.get(index)).copied() else { continue };
        for requested in [1e-3, -1e-3] {
            let mut probe = values.clone();
            let Some(slot) = probe.get_mut(&id).and_then(|v| v.get_mut(index)) else { break };
            *slot = q + requested;
            if let Some((lo, hi)) = joint.limits.get(index).copied().flatten() {
                *slot = (*slot).clamp(lo.min(hi), lo.max(hi));
            }
            let step = *slot - q;
            if step.abs() < 1e-10 {
                continue;
            }
            apply_links(asm, &mut probe);
            let next = if built.closing.is_empty() {
                tree.world(occurrence, &probe)
            } else {
                let mut changed = doc.clone();
                for j in &mut changed.assembly.joints {
                    if let Some(q) = probe.get(&j.id) {
                        j.values.clone_from(q);
                    }
                }
                let held = Held::from([((id, index), q)]);
                let solved = solve_holding_with_analysis(&changed, st, &held, Analysis::Placement);
                if solved.conflicts.is_empty() && solved.errors.is_empty() { solved.world.get(&occurrence).copied() } else { None }
            };
            let Some(next) = next else { continue };
            let delta = mat_mul(&next, &inverse);
            if let Some(axis) = axis_of(delta, step, reference, id, index) {
                // Coincident values describe the same visible motion; retain the nearest joint.
                if !out.iter().any(|a| {
                    a.angular == axis.angular
                        && a.axis.dot(axis.axis).abs() > 1.0 - 1e-6
                        && (!a.angular || (a.origin - axis.origin).cross(a.axis).len() < 1e-4)
                }) {
                    out.push(axis);
                }
                break;
            }
        }
    }
    out
}

fn axis_of(delta: Mat, step: f64, reference: Vec3, joint: u64, index: usize) -> Option<MotionAxis> {
    let (x, y, z) = (apply_vector(&delta, Vec3::X), apply_vector(&delta, Vec3::Y), apply_vector(&delta, Vec3::Z));
    let sine = Vec3::new(y.z - z.y, z.x - x.z, x.y - y.x);
    let angle = sine.len().atan2(x.x + y.y + z.z - 1.0);
    let translation = apply_point(&delta, Vec3::ZERO);
    let (origin, axis, angular, scale) = if angle > 1e-8 {
        let axis = sine.normalized()? * step.signum();
        let transverse = translation - axis * translation.dot(axis);
        let centre = (transverse + axis.cross(transverse) / (angle * step.signum() * 0.5).tan()) * 0.5;
        (centre + axis * reference.dot(axis), axis, true, angle / step.abs())
    } else {
        (reference, (translation / step).normalized()?, false, translation.len() / step.abs())
    };
    (origin.is_finite() && axis.is_finite() && scale.is_finite() && scale > 1e-8).then_some(MotionAxis { joint, index, origin, axis, angular, scale })
}
