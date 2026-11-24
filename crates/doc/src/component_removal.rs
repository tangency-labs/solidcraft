//! Removing a component tree: its components, the features made in them, their placements, and
//! every assembly record that refers to them.

use std::collections::BTreeSet;

use crate::{DocError, Document, FeatureKind, Result};

/// What [`Document::remove_component_tree`] removed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemovedTree {
    /// The removed components: the root and everything inside it.
    pub components: Vec<u64>,
    /// The features that were inside them, in timeline order.
    pub features: Vec<u64>,
    /// Every deleted feature: `features`, then features elsewhere built on their sketches.
    pub deleted: Vec<u64>,
    /// The removed occurrences: placements of the components and placements inside them.
    pub occurrences: Vec<u64>,
    /// The removed joints: those with either side on a removed occurrence.
    pub joints: Vec<u64>,
}

impl Document {
    /// Did feature `f` make the component tree it sits at the top of: an inserted file
    /// (`Feature::inserted`) or a linked design's bodies? Deleting it then removes that tree.
    pub fn feature_owns_component(&self, f: &crate::Feature) -> bool {
        f.component != 0
            && matches!(f.kind, FeatureKind::Import { .. } | FeatureKind::MeshImport { .. })
            && (f.inserted || self.links.get(&f.component).is_some_and(|link| link.feature == f.id))
    }

    /// Remove component `root` with every component inside it, the features made in them, their
    /// occurrences, the joints, motion links, joint origins, contact sets, motion keys and
    /// exploded moves that refer to those occurrences, the bodies' component assignments, and the
    /// components' standard-part, link, appearance and configuration records.
    ///
    /// Refused (nothing changes) for the design itself, an unknown component, or a tree that
    /// supplies a body transfer recorded outside it.
    pub fn remove_component_tree(&mut self, root: u64) -> Result<RemovedTree> {
        if root == 0 {
            return Err(DocError::Invalid("the design itself can't be deleted".into()));
        }
        if !self.components.iter().any(|c| c.id == root) {
            return Err(DocError::Unknown(format!("component {root}")));
        }
        let tree: BTreeSet<u64> = self.components.iter().filter(|c| self.component_within(c.id, root)).map(|c| c.id).collect();
        let kept = self.components_kept_by_outside_placements(root, &tree);
        let components: Vec<u64> = self.components.iter().map(|c| c.id).filter(|c| tree.contains(c) && !kept.contains(c)).collect();
        let inside: BTreeSet<u64> = components.iter().copied().collect();
        if self.features.iter().any(|feature| {
            !inside.contains(&feature.component)
                && matches!(&feature.kind, FeatureKind::BodyTransfer { from, to, .. } if inside.contains(from) || inside.contains(to))
        }) {
            return Err(DocError::Invalid(
                "component supplies a recorded body transfer outside this subtree; remove that transfer history before deleting its source".into(),
            ));
        }
        let features: Vec<u64> = self.features.iter().filter(|f| inside.contains(&f.component)).map(|f| f.id).collect();
        let deleted = if features.is_empty() { Vec::new() } else { self.delete_features(&features)? };

        let occurrences: BTreeSet<u64> =
            self.occurrences.iter().filter(|o| inside.contains(&o.component) || inside.contains(&o.parent)).map(|o| o.id).collect();
        // Linked parts this removal leaves without a placement: they go too.
        let orphans: Vec<u64> = self
            .links
            .keys()
            .copied()
            .filter(|c| !inside.contains(c))
            .filter(|c| {
                let mut placed = self.occurrences.iter().filter(|o| o.component == *c).peekable();
                placed.peek().is_some() && placed.all(|o| occurrences.contains(&o.id))
            })
            .collect();
        let joints = self.remove_occurrences(&occurrences);

        let bodies: BTreeSet<String> = self.body_components.iter().filter(|(_, c)| inside.contains(*c)).map(|(name, _)| name.clone()).collect();
        self.body_components.retain(|_, c| !inside.contains(c));
        self.body_offsets.retain(|name, _| !bodies.contains(name));
        // A kept component sat under the tree it was made in: it moves up to the component the
        // removed root sat in, which is left (read before the root goes).
        let up = self.components.iter().find(|c| c.id == root).map_or(0, |c| c.parent);
        self.components.retain(|c| !inside.contains(&c.id));
        for c in self.components.iter_mut().filter(|c| kept.contains(&c.id) && inside.contains(&c.parent)) {
            c.parent = up;
        }
        self.assembly.rigid_groups.retain(|g| !inside.contains(&g.component));
        self.parts.retain(|c, _| !inside.contains(c));
        self.gears.retain(|c, _| !inside.contains(c));
        self.links.retain(|c, _| !inside.contains(c));
        self.remove_configuration_components(&components);
        let mut removed =
            RemovedTree { components, features, deleted, occurrences: occurrences.into_iter().collect(), joints: joints.into_iter().collect() };
        for orphan in orphans {
            if !self.components.iter().any(|x| x.id == orphan) {
                continue;
            }
            let more = self.remove_component_tree(orphan)?;
            removed.components.extend(more.components);
            removed.features.extend(more.features);
            removed.deleted.extend(more.deleted);
            removed.occurrences.extend(more.occurrences);
            removed.joints.extend(more.joints);
        }
        Ok(removed)
    }

    /// The components of `tree` (the tree under `root`) that stay when it is removed: a part
    /// that was made inside it (a linked design's component) but that a component outside it
    /// still places, with everything inside such a part. They go with the last placement.
    fn components_kept_by_outside_placements(&self, root: u64, tree: &BTreeSet<u64>) -> BTreeSet<u64> {
        let mut kept: BTreeSet<u64> = BTreeSet::new();
        loop {
            let before = kept.len();
            let removed = |c: u64| tree.contains(&c) && !kept.contains(&c);
            let placed: Vec<u64> = self
                .occurrences
                .iter()
                .filter(|o| o.component != root && tree.contains(&o.component) && !removed(o.parent))
                .map(|o| o.component)
                .collect();
            kept.extend(placed);
            let inner: Vec<u64> = tree.iter().copied().filter(|c| kept.iter().any(|k| self.component_within(*c, *k))).collect();
            kept.extend(inner);
            if kept.len() == before {
                return kept;
            }
        }
    }

    /// Remove these occurrences and every assembly record that refers to them: the joints with
    /// either side on one (with those joints' motion links and motion keys), the joint origins
    /// on them, their places in contact sets and their exploded moves. Returns the joints removed.
    pub fn remove_occurrences(&mut self, occurrences: &BTreeSet<u64>) -> BTreeSet<u64> {
        let joints: BTreeSet<u64> = self
            .assembly
            .joints
            .iter()
            .filter(|j| j.a.occurrences().chain(j.b.occurrences()).any(|o| occurrences.contains(&o)))
            .map(|j| j.id)
            .collect();
        for group in &mut self.timeline_groups {
            group.joints.retain(|id| !joints.contains(id));
        }
        let assembly = &mut self.assembly;
        assembly.joints.retain(|j| !joints.contains(&j.id));
        assembly.links.retain(|l| !joints.contains(&l.a) && !joints.contains(&l.b));
        assembly.origins.retain(|o| !o.origin.occurrences().any(|o| occurrences.contains(&o)));
        for set in &mut assembly.contact_sets {
            set.occurrences.retain(|o| !occurrences.contains(o));
        }
        for study in &mut assembly.studies {
            study.keys.retain(|k| !joints.contains(&k.joint));
        }
        for view in &mut assembly.exploded {
            view.moves.retain(|(o, _)| !occurrences.contains(o));
        }
        for group in &mut assembly.rigid_groups {
            group.members.retain(|o| !occurrences.contains(o));
        }
        self.occurrences.retain(|o| !occurrences.contains(&o.id));
        joints
    }
}
