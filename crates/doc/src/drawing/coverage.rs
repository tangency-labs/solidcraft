//! Which feature a drawing's model reference covers (#13054): the latest feature in the timeline
//! among those that made the faces it names. It is read from the saved persistent names alone, so
//! a lost reference still covers what it named, and nothing is evaluated.
//!
//! - An edge covers the latest feature among the faces it separates: that feature made the edge.
//!   A hole's rim (the block's top and the hole's wall) covers the hole.
//! - A corner covers the latest feature among the faces of its edges.
//! - A face covers the feature that made it.
//!
//! A pattern's or mirror's copy of a face is made by the pattern ([`FaceName::feature`]), and the
//! faces that tell the pieces of a split face apart don't count. A face of no feature (a body's
//! face by index, or a name of no known form) adds nothing. A reference that names a feature the
//! timeline no longer has covers nothing: the feature that made what it named is gone.

use std::collections::HashMap;

use super::dimension::Reference;
use crate::Document;
use crate::persistent_name::{EdgeName, FaceName};

/// The design's timeline order, read once for a request: each feature id by its place.
pub struct Timeline(HashMap<u64, usize>);

impl Timeline {
    pub fn of(doc: &Document) -> Timeline {
        Timeline(doc.features.iter().enumerate().map(|(at, f)| (f.id, at)).collect())
    }

    /// The feature `reference` covers, by the names it keeps, if any.
    pub fn covers(&self, reference: Reference) -> Option<u64> {
        let edge_faces = |name: &str| EdgeName::parse(name).map(|edge| edge.faces).unwrap_or_default();
        let faces: Vec<FaceName> = match reference {
            Reference::Edge(edge) => edge_faces(&edge.name),
            Reference::Point(point) => point.edges.iter().flat_map(|name| edge_faces(name)).collect(),
            Reference::Face(face) => FaceName::parse(&face.name).into_iter().collect(),
        };
        let mut latest: Option<(usize, u64)> = None;
        for feature in faces.iter().filter_map(FaceName::feature) {
            let at = *self.0.get(&feature.0)?;
            latest = latest.max(Some((at, feature.0)));
        }
        latest.map(|(_, feature)| feature)
    }
}

#[cfg(test)]
#[path = "coverage_tests.rs"]
mod tests;
