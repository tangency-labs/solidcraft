//! References from a feature to the rest of the design: the sketches it is built from (by id),
//! the features it names (patterned and mirrored features, the construction plane a plane is
//! built on) and the bodies it works on (by name). The walk (`crate::walk`) is behind
//! `input_sketches`, `remap_refs` and `remap_bodies`.

use std::collections::BTreeMap;

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

    /// The bodies the feature names (the bodies it works on), in the order it holds them.
    pub fn body_refs(&self) -> Vec<String> {
        let mut v = Vec::new();
        self.walk_refs(&mut |r| {
            if let Ref::Body(name) = r {
                v.push(name.as_ref().to_string());
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
