//! Rigid groups: a component whose contents move as one (an imported assembly's). One record
//! holds every occurrence inside the component where it is in its parent, however many there
//! are, where a rigid joint per placement used to. The joint solver, mass properties and URDF
//! export see the record as those rigid joints ([`Document::rigid_group_joints`]); the joint
//! list shows it as one row.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

use super::{Joint, JointKind, JointOrigin, Snap};
use crate::{Document, apply_point, apply_vector};

/// A component whose contents stay where they are in it: every occurrence inside it, at any
/// depth, keeps its placement in its parent, as a rigid joint to that parent would keep it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RigidGroup {
    pub id: u64,
    pub name: String,
    /// The component whose contents it holds.
    pub component: u64,
}

impl Document {
    /// The rigid group holding `component`'s own contents, if any.
    pub fn rigid_group_of(&self, component: u64) -> Option<&RigidGroup> {
        if component == 0 {
            return None;
        }
        self.assembly.rigid_groups.iter().find(|g| g.component == component)
    }

    /// The rigid group whose contents include occurrence `occ` (it sits in the group's component
    /// or in a component inside it), if any.
    pub fn rigid_group_holding(&self, occ: u64) -> Option<&RigidGroup> {
        let parent = self.occurrences.iter().find(|o| o.id == occ)?.parent;
        if parent == 0 {
            return None;
        }
        self.assembly.rigid_groups.iter().find(|g| g.component != 0 && self.component_within(parent, g.component))
    }

    /// The occurrences a rigid group holds: every occurrence inside its component, at any depth.
    pub fn rigid_group_members(&self, group: &RigidGroup) -> Vec<u64> {
        if group.component == 0 {
            return Vec::new();
        }
        self.occurrences.iter().filter(|o| o.parent != 0 && self.component_within(o.parent, group.component)).map(|o| o.id).collect()
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
        for group in &self.assembly.rigid_groups {
            if group.component == 0 {
                continue;
            }
            for o in self.occurrences.iter().filter(|o| o.parent != 0 && self.component_within(o.parent, group.component)) {
                // An occurrence held by an earlier group is held once.
                if self
                    .assembly
                    .rigid_groups
                    .iter()
                    .take_while(|g| g.id != group.id)
                    .any(|g| g.component != 0 && self.component_within(o.parent, g.component))
                {
                    continue;
                }
                let Some(a) = self.occurrence_of(o.parent).map(|p| p.id) else { continue };
                while taken.contains(&next) {
                    next = next.saturating_sub(1);
                }
                let m = &o.transform;
                out.push(Joint {
                    id: next,
                    name: group.name.clone(),
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
