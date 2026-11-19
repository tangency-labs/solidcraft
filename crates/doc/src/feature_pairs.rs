//! The features of two components with one history, paired ([`FeaturePairs::between`]), and
//! references to one component's bodies moved onto the other's through the pairs.
//!
//! A face or edge name says which features made it (`F12:side:l4`), and a reference also names its
//! body and the feature that made the body. A component and its Paste New copy, or two components
//! built the same way, have the same features in the same order, but with other ids, and their
//! bodies may have other names. Component paste builds the two maps, features and bodies, while it
//! copies, and rewrites names through them with [`crate::remap_topology`], but it doesn't keep
//! them. These are the same two maps, worked out again from the two timelines, so every rewrite
//! that takes them takes these. A name is rewritten through the name grammar
//! ([`crate::persistent_name`]), each face of an edge on its own, so a feature with no pair is
//! found wherever it sits.
//!
//! Nothing here looks the moved reference up: whether it still finds one edge on the other
//! component is the caller's to ask ([`crate::Document::finds_one`]), as a repair asks.

use std::collections::BTreeMap;

use crate::drawing::dimension::{ModelEdge, ModelFace, ModelPoint};
use crate::persistent_name::{Base, EdgeName, FeatureId};
use crate::{Document, Feature, ModelState};

/// One component of one design, with the design's evaluated model.
#[derive(Clone, Copy)]
pub struct Side<'a> {
    pub doc: &'a Document,
    pub state: &'a ModelState,
    pub component: u64,
}

impl<'a> Side<'a> {
    /// The component's own features, in timeline order.
    fn features(self) -> impl Iterator<Item = &'a Feature> {
        self.doc.features.iter().filter(move |f| f.component == self.component)
    }

    /// The names of the bodies `feature` makes, in the order the model lists them.
    fn bodies_of(self, feature: u64) -> impl Iterator<Item = String> + 'a {
        self.state.bodies.iter().filter(move |b| b.feature == feature).map(|b| b.name.clone())
    }
}

/// Each feature of one component and the one of another it pairs with, and the bodies they make.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeaturePairs {
    pub features: BTreeMap<u64, u64>,
    pub bodies: BTreeMap<String, String>,
}

impl FeaturePairs {
    /// `from`'s own features and `to`'s, paired in timeline order while their kinds match, up to
    /// the first pair that doesn't: past it the two histories part, so nothing later pairs. The
    /// bodies each pair makes are paired in the order the model lists them. One component on both
    /// sides of one design pairs every feature and body with itself.
    pub fn between(from: Side<'_>, to: Side<'_>) -> FeaturePairs {
        let mut pairs = FeaturePairs::default();
        for (a, b) in from.features().zip(to.features()) {
            if a.kind.base_name() != b.kind.base_name() {
                break;
            }
            pairs.features.insert(a.id, b.id);
            pairs.bodies.extend(from.bodies_of(a.id).zip(to.bodies_of(b.id)));
        }
        pairs
    }

    /// `edge` of a body placed through occurrence path `from`, as the edge of the other component
    /// placed through `to` that the pairs make of it. `None` when it is placed through another path
    /// (a part inside `from` isn't moved), or its body, the feature that made it or a feature its
    /// name names has no pair.
    pub fn edge(&self, edge: &ModelEdge, from: &[u64], to: &[u64]) -> Option<ModelEdge> {
        let (body, origin_feature, path) = self.place(&edge.body, edge.origin_feature, &edge.path, from, to)?;
        Some(ModelEdge { body, origin_feature, name: self.name(&edge.name)?, path })
    }

    /// As [`FeaturePairs::edge`], for a face.
    pub fn face(&self, face: &ModelFace, from: &[u64], to: &[u64]) -> Option<ModelFace> {
        let (body, origin_feature, path) = self.place(&face.body, face.origin_feature, &face.path, from, to)?;
        Some(ModelFace { body, origin_feature, name: self.name(&face.name)?, path })
    }

    /// As [`FeaturePairs::edge`], for a corner or a centre: every edge it names.
    pub fn point(&self, point: &ModelPoint, from: &[u64], to: &[u64]) -> Option<ModelPoint> {
        let (body, origin_feature, path) = self.place(&point.body, point.origin_feature, &point.path, from, to)?;
        let edges = point.edges.iter().map(|name| self.name(name)).collect::<Option<Vec<_>>>()?;
        Some(ModelPoint { body, origin_feature, edges, centre: point.centre, path })
    }

    /// A reference's body, the feature that made it and its path, moved from `from` to `to`.
    fn place(&self, body: &str, feature: u64, path: &[u64], from: &[u64], to: &[u64]) -> Option<(String, u64, Vec<u64>)> {
        if path != from {
            return None;
        }
        Some((self.bodies.get(body)?.clone(), *self.features.get(&feature)?, to.to_vec()))
    }

    /// `name` with each feature and body face through the pairs, read by the name grammar face by
    /// face: `None` when a feature it names has no pair, as one made after the histories part names
    /// nothing on the other component. A face of no form the grammar knows (an older design's
    /// name, a sheet's flat face) goes to [`crate::remap_topology`], which reads those forms.
    fn name(&self, name: &str) -> Option<String> {
        let edge = match EdgeName::parse(name) {
            Ok(edge) if !edge.faces.iter().any(|face| matches!(face.base, Base::Other(_))) => edge,
            _ => return Some(crate::remap_topology(name, &self.features, &self.bodies)),
        };
        let mut feature = |id: FeatureId, _| self.features.get(&id.0).map(|n| FeatureId(*n));
        let mut body = |b: &str| self.bodies.get(b).cloned();
        let faces = edge.faces.iter().map(|face| face.map(&mut feature, &mut body)).collect::<Option<Vec<_>>>()?;
        Some(EdgeName { faces, tie: edge.tie }.to_string())
    }
}

#[cfg(test)]
#[path = "feature_pairs_tests.rs"]
mod tests;
