//! References from a feature to the rest of the design: the sketches it is built from (by id),
//! the features it names (patterned and mirrored features and the construction plane a plane is
//! built on, by id since #1134; construction axes and points, still by name) and the bodies it
//! works on (by name). The walk (`crate::walk`) is behind
//! `input_sketches`, `remap_refs` and `remap_bodies`.

use std::collections::BTreeMap;

use crate::walk::Walk;
use crate::{BodyName, FeatureId, FeatureKind, FeatureName, SketchId};

/// A reference a feature holds: `S` is `&SketchId` or `&mut SketchId`, and so on. `Feature`
/// carries a feature id (pattern and mirror seeds, construction planes); `FeatureName` carries
/// a feature name, for the references that are still keyed by name (construction axes and
/// points).
pub(crate) enum Ref<S, F, N, B> {
    Sketch(S),
    Feature(F),
    FeatureName(N),
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

    /// Whether the feature refers to the feature `id` (by id: a patterned or mirrored feature,
    /// the construction plane a plane is built on) or names it (`name`: a construction axis or
    /// point it is built from). Used to refuse a feature built on itself (#3567).
    pub fn names_feature(&self, id: u64, name: &str) -> bool {
        let mut found = false;
        self.walk_refs(&mut |r| match r {
            Ref::Feature(x) => found |= x.0 == id,
            Ref::FeatureName(n) => found |= n.as_ref() == name,
            _ => {}
        });
        found
    }

    /// Bodies the feature works on (by name).
    pub fn input_bodies(&self) -> Vec<String> {
        let mut v = Vec::new();
        self.walk_refs(&mut |r| {
            if let Ref::Body(name) = r {
                v.push(name.as_ref().to_string());
            }
        });
        v
    }

    /// Point references to sketches and features (both by id) at their copies, and references
    /// that are still keyed by name at a feature's new name (`timeline.rename`).
    pub fn remap_refs(&mut self, ids: &BTreeMap<u64, u64>, names: &BTreeMap<String, String>) {
        self.walk_refs_mut(&mut |r| match r {
            Ref::Sketch(id) => {
                if let Some(n) = ids.get(&id.0) {
                    *id = SketchId(*n);
                }
            }
            Ref::Feature(id) => {
                if let Some(n) = ids.get(&id.0) {
                    *id = FeatureId(*n);
                }
            }
            Ref::FeatureName(name) => {
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
    /// references (`remap_refs`): sketch projections of a construction plane (#4353). The
    /// features are the rename command's to change; feature references inside features are keyed
    /// by id since #1134 and follow a rename on their own.
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
    }
}
