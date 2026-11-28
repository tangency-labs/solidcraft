//! Where a Combine puts its tool (#23542): the assembly constraints of its placement, each kept as
//! the joint its rule maps to (`Constraint::fit`), solved by the joint solver as a two-part
//! assembly of its own. The target is fixed where it is and the tool is free; the first joint
//! places the tool and the others close loops, as several constraints between one pair of parts
//! do in an assembly. Each joint's values start where the tool is now, so the directions the rules
//! leave free keep the tool where it is.

use std::collections::BTreeMap;

use super::relations::values_between;
use super::{Assembly, CONFLICT_MM, CONFLICT_RAD, Held, Joint, JointFrames, Tree, Values, close_loops, loop_dof, mismatch};
use crate::{DocError, Mat, Result, mat_mul};

/// The target's side in a placement's joints: it stays where it is.
pub const TARGET: u64 = 1;
/// The tool's side: the joints place it.
pub const TOOL: u64 = 2;

/// One rule of a placement as a joint between [`TARGET`] and [`TOOL`] (either way round, as the
/// rule's fit puts the plane or axis on side a), with its joint frames in each side's body
/// coordinates: A's (`a`, without the lateral offsets) and B's (`b`).
#[derive(Clone, Debug)]
pub struct PlacementJoint {
    pub joint: Joint,
    pub a: Mat,
    pub b: Mat,
}

/// Where the tool goes, and how many independent directions the rules leave it (0: fixed).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ToolPlacement {
    pub world: Mat,
    pub dof: usize,
}

/// Place the tool by its rules: the target's body is at `target` and the tool's at `tool` (world
/// transforms of their bodies' coordinates), and `size` is the tool's size, which a turn weighs as.
/// Refused when there are no rules, when a rule's values can't be found from where the tool is, or
/// when the rules disagree (a conflict, named by the rule).
pub fn place_tool(target: &Mat, tool: &Mat, size: f64, rules: &[PlacementJoint]) -> Result<ToolPlacement> {
    let first = rules.first().ok_or_else(|| DocError::Invalid("a placement needs at least one constraint".into()))?;
    if rules.iter().any(|r| {
        ![TARGET, TOOL].contains(&r.joint.a.occurrence)
            || ![TARGET, TOOL].contains(&r.joint.b.occurrence)
            || r.joint.a.occurrence == r.joint.b.occurrence
    }) {
        return Err(DocError::Invalid("each placement constraint joins the target and the tool".into()));
    }
    let side = |occurrence: u64| if occurrence == TARGET { *target } else { *tool };
    // Every rule's values where the tool is now.
    let mut vals = Values::new();
    for r in rules {
        let fa = mat_mul(&side(r.joint.a.occurrence), &r.joint.offset_frame_a(r.a));
        let fb = mat_mul(&side(r.joint.b.occurrence), &r.b);
        let mut q = values_between(&r.joint, &fa, &fb, size)
            .ok_or_else(|| DocError::Invalid(format!("{}: its references can't be found where the tool is", r.joint.name)))?;
        q.resize(r.joint.kind.dofs().len(), 0.0);
        vals.insert(r.joint.id, q);
    }
    let tree = Tree {
        joints: rules.iter().map(|r| (r.joint.id, &r.joint)).collect(),
        frames: rules.iter().map(|r| (r.joint.id, JointFrames::new(r.joint.offset_frame_a(r.a), r.b))).collect(),
        fixed: BTreeMap::from([(TARGET, *target)]),
        parent: BTreeMap::from([(TOOL, (TARGET, first.joint.id, first.joint.a.occurrence == TARGET))]),
        order: vec![TOOL],
    };
    let closing: Vec<u64> = rules.iter().skip(1).map(|r| r.joint.id).collect();
    let asm = Assembly { joints: rules.iter().map(|r| r.joint.clone()).collect(), ..Assembly::default() };
    if !closing.is_empty() {
        close_loops(&tree, &closing, &asm, &Held::new(), &mut vals);
    }
    let placed = tree.place(&vals);
    let world = *placed.get(&TOOL).ok_or_else(|| DocError::Invalid(format!("{} can't place the tool", first.joint.name)))?;
    // Each loop-closing rule checked as the solver checks a joint that closes a loop.
    let mut off = Vec::new();
    for id in &closing {
        let (Some(j), Some(frames)) = (tree.joints.get(id), tree.frames.get(id)) else { continue };
        let (Some(ta), Some(tb)) = (placed.get(&j.a.occurrence), placed.get(&j.b.occurrence)) else { continue };
        let Some(want) = tree.step(*id, true, ta, &vals) else { continue };
        let (dt, da) = mismatch(&mat_mul(&want, &frames.b), &mat_mul(tb, &frames.b));
        if dt > CONFLICT_MM || da > CONFLICT_RAD {
            off.push(format!("{} (off by {dt:.4} mm and {:.3}°)", j.name, da.to_degrees()));
        }
    }
    if !off.is_empty() {
        return Err(DocError::Invalid(format!("the placement's constraints disagree: {} can't be met with the others", off.join(", "))));
    }
    let mut dof = BTreeMap::from([(TARGET, 0), (TOOL, first.joint.kind.dofs().len())]);
    if !closing.is_empty() {
        loop_dof(&tree, &closing, &asm, &vals, &mut dof);
    }
    Ok(ToolPlacement { world, dof: dof.get(&TOOL).copied().unwrap_or(0) })
}
