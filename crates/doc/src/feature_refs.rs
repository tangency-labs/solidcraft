//! References from a feature to the rest of the design: the sketches it is built from (by id),
//! the features it names (patterned and mirrored features, the construction plane a plane is
//! built on) and the bodies it works on (by name). The walk (`crate::walk`) is behind
//! `input_sketches`, `remap_refs` and `remap_bodies`.

use std::collections::{BTreeMap, BTreeSet};

use crate::walk::Walk;
use crate::{BodyName, FeatureKind, FeatureName, SketchId};

/// A reference a feature holds: `S` is `&SketchId` or `&mut SketchId`, and so on.
pub(crate) enum Ref<S, F, B> {
    Sketch(S),
    Feature(F),
    Body(B),
}

impl FeatureKind {
    /// Sketches the feature is built from (profiles, paths, points): deleting one of them
    /// deletes the feature.
    pub fn input_sketches(&self) -> Vec<u64> {
        let mut v = Vec::new();
        self.walk_refs(&mut |r| {
            if let Ref::Sketch(id) = r {
                v.push(id.0);
            }
        });
        v
    }

    /// Whether the feature names the feature `name` (the construction plane a plane is built on,
    /// a patterned or mirrored feature).
    pub fn names_feature(&self, name: &str) -> bool {
        let mut found = false;
        self.walk_refs(&mut |r| {
            if let Ref::Feature(n) = r {
                found |= n.as_ref() == name;
            }
        });
        found
    }

    /// Point references to sketches (by id) and features (by name) at their copies, or at a
    /// feature's new name (`timeline.rename`).
    pub fn remap_refs(&mut self, sketches: &BTreeMap<u64, u64>, names: &BTreeMap<String, String>) {
        self.walk_refs_mut(&mut |r| match r {
            Ref::Sketch(id) => {
                if let Some(n) = sketches.get(&id.0) {
                    *id = SketchId(*n);
                }
            }
            Ref::Feature(name) => {
                if let Some(n) = names.get(name.as_ref()) {
                    *name = FeatureName(n.clone());
                }
            }
            Ref::Body(_) => {}
        });
    }

    /// Point references to bodies (by name) at their copies.
    pub fn remap_bodies(&mut self, bodies: &BTreeMap<String, String>) {
        self.walk_refs_mut(&mut |r| {
            if let Ref::Body(name) = r
                && let Some(n) = bodies.get(name.as_ref())
            {
                *name = BodyName(n.clone());
            }
        });
    }
}

impl crate::Document {
    /// Rename feature `old` to `new` where the design names it outside the features' own
    /// references (`remap_refs`): sketch projections of a construction plane and the canvases
    /// placed on it (#4353). (The features are the rename command's to change.)
    pub fn rename_feature_keys(&mut self, old: &str, new: &str) {
        let names = BTreeMap::from([(old.to_string(), new.to_string())]);
        let (ids, bodies) = (BTreeMap::new(), BTreeMap::new());
        for f in &mut self.features {
            if let FeatureKind::Sketch { sketch, .. } = &mut f.kind {
                for link in &mut sketch.links {
                    crate::clipboard::remap_link(&mut link.source, &ids, &names, &bodies, 0);
                }
            }
        }
        for c in &mut self.canvases {
            c.plane.walk_refs_mut(&mut |r| {
                if let Ref::Feature(name) = r
                    && name.as_ref() == old
                {
                    *name = FeatureName(new.to_string());
                }
            });
        }
    }
}

impl crate::Document {
    /// The features that affect component `component` (#5854): its own and its sub-components',
    /// what they are built from, followed back (the sketches, the features they name, the
    /// features that made the bodies they work on, targets and participants alike), and every
    /// feature that works on one of its bodies. `None` for the root, where every feature does. A walk of the design's references
    /// and of which feature made each body in `st` and whose it is: nothing is evaluated.
    pub fn features_affecting(&self, st: &crate::ModelState, component: u64) -> Option<BTreeSet<u64>> {
        if component == 0 {
            return None;
        }
        let inside = |mut c: u64| {
            for _ in 0..64 {
                if c == component {
                    return true;
                }
                if c == 0 {
                    return false;
                }
                c = self.components.iter().find(|k| k.id == c).map_or(0, |k| k.parent);
            }
            false
        };
        let made_by: BTreeMap<&str, u64> = st.bodies.iter().map(|b| (b.name.as_str(), b.feature)).collect();
        let own_bodies: BTreeSet<&str> = st.bodies.iter().filter(|b| inside(self.model_body_component(b))).map(|b| b.name.as_str()).collect();
        let mut affecting: BTreeSet<u64> = self.features.iter().filter(|f| inside(f.component)).map(|f| f.id).collect();
        // A feature elsewhere that works on one of the component's bodies (a cut from the root).
        for f in &self.features {
            let mut on_own = f.participants.iter().any(|b| own_bodies.contains(b.as_str()));
            f.kind.walk_refs(&mut |r| {
                if let Ref::Body(b) = r {
                    on_own |= own_bodies.contains(b.as_ref());
                }
            });
            if on_own {
                affecting.insert(f.id);
            }
        }
        let mut queue: Vec<u64> = affecting.iter().copied().collect();
        while let Some(id) = queue.pop() {
            let Some(f) = self.feature(id) else { continue };
            let mut inputs: Vec<u64> = f.participants.iter().filter_map(|b| made_by.get(b.as_str()).copied()).collect();
            f.kind.walk_refs(&mut |r| match r {
                Ref::Sketch(s) => inputs.push(s.0),
                Ref::Feature(n) => inputs.extend(self.find_feature(n.as_ref()).map(|g| g.id)),
                Ref::Body(b) => inputs.extend(made_by.get(b.as_ref()).copied()),
            });
            for i in inputs {
                if self.feature(i).is_some() && affecting.insert(i) {
                    queue.push(i);
                }
            }
        }
        Some(affecting)
    }
}
