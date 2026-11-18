//! Rigid groups: the parts an import placed (an imported assembly's), held where they are in
//! their parents by one record however many there are, where a rigid joint per placement used to.
//! The joint solver, mass properties and URDF export see the record as those rigid joints
//! ([`Document::rigid_group_joints`]); the joint list shows it as one row.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

use super::{Joint, JointKind, JointOrigin, Snap};
use crate::{Document, apply_point, apply_vector};

/// Occurrences that each keep their placement in their parent, as a rigid joint to the parent
/// would keep them: the parts an import made inside `component`. A part placed inside the
/// component later is not a member, so it moves on its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RigidGroup {
    pub id: u64,
    pub name: String,
    /// The component whose parts it holds (`component.unlock` on it releases them).
    pub component: u64,
    /// The occurrences it holds, by id.
    #[serde(default)]
    pub members: Vec<u64>,
}

impl Document {
    /// The rigid group of `component` (the one `component.unlock` on it releases), if any.
    pub fn rigid_group_of(&self, component: u64) -> Option<&RigidGroup> {
        if component == 0 {
            return None;
        }
        self.assembly.rigid_groups.iter().find(|g| g.component == component)
    }

    /// The rigid group holding occurrence `occ` where it is in its parent, if any.
    pub fn rigid_group_holding(&self, occ: u64) -> Option<&RigidGroup> {
        self.assembly.rigid_groups.iter().find(|g| g.members.contains(&occ))
    }

    /// The occurrences a rigid group holds that are still in the design.
    pub fn rigid_group_members(&self, group: &RigidGroup) -> Vec<u64> {
        let live: BTreeSet<u64> = self.occurrences.iter().filter(|o| o.parent != 0).map(|o| o.id).collect();
        group.members.iter().copied().filter(|m| live.contains(m)).collect()
    }

    /// The rigid groups as the rigid joints they stand for: one per occurrence they hold, from
    /// the first placement of the component it sits in to it, as built (B's origin seen from A,
    /// which is its placement there). Ids count down from `u64::MAX`, skipping the design's own
    /// joint ids; each is named after its group. Built anew from the placements each time.
    pub fn rigid_group_joints(&self) -> Vec<Joint> {
        if self.assembly.rigid_groups.is_empty() {
            return Vec::new();
        }
        let taken: BTreeSet<u64> = self.assembly.joints.iter().map(|j| j.id).collect();
        let mut next = u64::MAX;
        let mut out = Vec::new();
        let mut held = BTreeSet::new();
        let by_id: std::collections::BTreeMap<u64, &crate::Occurrence> = self.occurrences.iter().map(|o| (o.id, o)).collect();
        let mut first_of: std::collections::BTreeMap<u64, u64> = std::collections::BTreeMap::new();
        for o in &self.occurrences {
            first_of.entry(o.component).or_insert(o.id);
        }
        for group in &self.assembly.rigid_groups {
            for o in group.members.iter().filter_map(|m| by_id.get(m).copied()) {
                // An occurrence held by two groups is held once.
                if o.parent == 0 || !held.insert(o.id) {
                    continue;
                }
                // The first placement of the component it sits in, as `occurrence_of` finds it.
                let Some(&a) = first_of.get(&o.parent) else { continue };
                while taken.contains(&next) {
                    next = next.saturating_sub(1);
                }
                let m = &o.transform;
                out.push(Joint {
                    id: next,
                    // One name per occurrence; URDF export suffixes any that still coincide, one step each.
                    name: format!("{}: {}", group.name, o.name),
                    kind: JointKind::Rigid,
                    a: JointOrigin::new(
                        a,
                        Snap::Frame { origin: apply_point(m, Vec3::ZERO), z: apply_vector(m, Vec3::Z), x: Some(apply_vector(m, Vec3::X)) },
                    ),
                    b: JointOrigin::new(o.id, Snap::Frame { origin: Vec3::ZERO, z: Vec3::Z, x: Some(Vec3::X) }),
                    values: Vec::new(),
                    limits: Vec::new(),
                    flip: true,
                    offset: 0.0,
                    angle: 0.0,
                    suppressed: false,
                    exprs: Default::default(),
                    offset_xy: [0.0; 2],
                    captured: None,
                    constraint: None,
                });
                next = next.saturating_sub(1);
            }
        }
        out
    }

    /// The design's joints and, after them, the rigid joints its rigid groups stand for: what
    /// places its occurrences.
    pub fn joints_with_rigid_groups(&self) -> std::borrow::Cow<'_, [Joint]> {
        let held = self.rigid_group_joints();
        if held.is_empty() {
            return std::borrow::Cow::Borrowed(&self.assembly.joints);
        }
        let mut all = self.assembly.joints.clone();
        all.extend(held);
        std::borrow::Cow::Owned(all)
    }
}
