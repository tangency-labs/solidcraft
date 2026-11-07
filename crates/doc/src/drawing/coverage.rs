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

use super::dimension::DimensionKind;
use super::note::Anchor;
use crate::Document;
use crate::persistent_name::{EdgeName, FaceName};

/// A saved model reference, by the persistent names it holds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Reference<'a> {
    /// An edge's name.
    Edge(&'a str),
    /// The names of the edges that meet at a corner, or of a circle whose centre it is.
    Corner(&'a [String]),
    /// A face's name.
    Face(&'a str),
}

impl DimensionKind {
    /// Each model reference it measures, in its order.
    pub fn references(&self) -> Vec<Reference<'_>> {
        match self {
            DimensionKind::Linear { a, b, .. } => vec![Reference::Corner(&a.edges), Reference::Corner(&b.edges)],
            DimensionKind::Length { edge, .. } | DimensionKind::Radial { edge, .. } => vec![Reference::Edge(&edge.name)],
            DimensionKind::Angle { a, b, .. } => vec![Reference::Edge(&a.name), Reference::Edge(&b.name)],
            DimensionKind::Chain { points, .. } => points.iter().map(|point| Reference::Corner(&point.edges)).collect(),
            DimensionKind::Cylinder { face } | DimensionKind::Sphere { face } | DimensionKind::Cone { face } => vec![Reference::Face(&face.name)],
            DimensionKind::Silhouettes { a, b, .. } => vec![Reference::Face(&a.face.name), Reference::Face(&b.face.name)],
            DimensionKind::Faces { a, b, .. } => vec![Reference::Face(&a.name), Reference::Face(&b.name)],
        }
    }
}

impl Anchor {
    /// The model reference its leader points at, if it points at one.
    pub fn reference(&self) -> Option<Reference<'_>> {
        match self {
            Anchor::Sheet(_) | Anchor::View(_) => None,
            Anchor::Edge { edge, .. } => Some(Reference::Edge(&edge.name)),
            Anchor::Corner { point, .. } => Some(Reference::Corner(&point.edges)),
        }
    }
}

/// The design's timeline order, read once for a request: each feature id by its place.
pub struct Timeline(HashMap<u64, usize>);

impl Timeline {
    pub fn of(doc: &Document) -> Timeline {
        Timeline(doc.features.iter().enumerate().map(|(at, f)| (f.id, at)).collect())
    }

    /// The feature `reference` covers, if any.
    pub fn covers(&self, reference: &Reference) -> Option<u64> {
        let edge_faces = |name: &str| EdgeName::parse(name).map(|edge| edge.faces).unwrap_or_default();
        let faces: Vec<FaceName> = match reference {
            Reference::Edge(name) => edge_faces(name),
            Reference::Corner(edges) => edges.iter().flat_map(|name| edge_faces(name)).collect(),
            Reference::Face(name) => FaceName::parse(name).into_iter().collect(),
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
