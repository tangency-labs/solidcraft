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
//!
//! A feature's coverage ([`of`]) is then, in this order: not applicable when it is suppressed or
//! rolled back; covered when a dimension or note covers it; not covered when a view draws a face it
//! made, named from exact history; unknown when the faces a view draws of it aren't; not applicable
//! when only reference parts draw it, or no view draws a face it made.

use std::collections::{HashMap, HashSet};

use super::dimension::Reference;
use crate::persistent_name::{Base, EdgeName, FaceName, FeatureId, Role};
use crate::{Document, ModelBody, naming};

/// Where a feature stands with a drawing's dimensions and notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Covered,
    NotCovered,
    Unknown,
    NotApplicable,
}

/// One feature of the design, its [`State`], the annotations that cover it, and why it is unknown or
/// not applicable.
#[derive(Clone, Debug, PartialEq)]
pub struct FeatureCoverage<A> {
    pub feature: u64,
    pub name: String,
    pub state: State,
    pub by: Vec<A>,
    pub why: Option<&'static str>,
}

/// The features that made the faces the views draw, by how each face is named.
#[derive(Debug, Default)]
pub struct Drawn {
    /// From exact history.
    exact: HashSet<u64>,
    /// Worked out from geometry, or with no feature in the name (the body's feature, then).
    inexact: HashSet<u64>,
    /// On a reference part only.
    reference: HashSet<u64>,
}

impl Drawn {
    /// Each face of `body`, which a view draws: as a reference part's when `reference` is.
    pub fn body(&mut self, body: &ModelBody, reference: bool) {
        for (i, name) in naming::face_names(body).iter().enumerate() {
            let maker = made_by(name);
            let set = match maker {
                _ if reference => &mut self.reference,
                Some(_) if naming::face_name_exact(body, i) => &mut self.exact,
                _ => &mut self.inexact,
            };
            set.insert(maker.unwrap_or(body.feature));
        }
    }
}

/// Each feature of `doc`, in timeline order, by the module's rules: covered by those of
/// `annotations` (each with its model references) that point at a face it made, else by what the
/// views draw of it (`drawn`). Each annotation's names are read once.
pub fn of<'r, A: Clone>(doc: &Document, annotations: impl IntoIterator<Item = (A, Vec<Reference<'r>>)>, drawn: &Drawn) -> Vec<FeatureCoverage<A>> {
    let timeline = Timeline::of(doc);
    let mut by: HashMap<u64, Vec<A>> = HashMap::new();
    for (annotation, references) in annotations {
        let mut features: Vec<u64> = references.into_iter().filter_map(|r| timeline.covers(r)).collect();
        features.sort_unstable();
        features.dedup();
        for feature in features {
            by.entry(feature).or_default().push(annotation.clone());
        }
    }
    let rolled_back = doc.marker.unwrap_or(usize::MAX);
    let mut out = Vec::with_capacity(doc.features.len());
    for (at, f) in doc.features.iter().enumerate() {
        let by = by.remove(&f.id).unwrap_or_default();
        let (state, why) = if f.suppressed {
            (State::NotApplicable, Some("it is suppressed"))
        } else if at >= rolled_back {
            (State::NotApplicable, Some("it is rolled back"))
        } else if !by.is_empty() {
            (State::Covered, None)
        } else if drawn.exact.contains(&f.id) {
            (State::NotCovered, None)
        } else if drawn.inexact.contains(&f.id) {
            (State::Unknown, Some("the faces a view draws of it aren't named from exact history, so what it made can't be told"))
        } else if drawn.reference.contains(&f.id) {
            (State::NotApplicable, Some("only reference parts draw it"))
        } else {
            (State::NotApplicable, Some("no view draws a face it made"))
        };
        out.push(FeatureCoverage { feature: f.id, name: f.name.clone(), state, by, why });
    }
    out
}

/// The feature that made the face named `name`, if its name says ([`FaceName::feature`]: a copy's
/// is the pattern's or mirror's).
pub fn made_by(name: &str) -> Option<u64> {
    face_maker(name).map(|feature| feature.0)
}

/// [`made_by`], as the feature's id.
pub(crate) fn face_maker(name: &str) -> Option<FeatureId> {
    FaceName::parse(name).ok()?.feature()
}

/// The features that made the faces an edge's name says it separates.
pub(crate) fn edge_makers(name: &str) -> Vec<FeatureId> {
    EdgeName::parse(name).map_or_else(|_| Vec::new(), |edge| edge.faces.iter().filter_map(FaceName::feature).collect())
}

/// Whether the face named `name` is a whole cap of feature `feature`: an extrude's face at its start
/// or its end, or on the top or bottom of its sketch's plane, by the roles the extrude names its caps
/// with (`lineage::extrude_cap_role`).
pub fn is_cap(name: &str, feature: u64) -> bool {
    match FaceName::parse(name) {
        Ok(FaceName { base: Base::Made { feature: by, role: Role::Start | Role::End | Role::Top | Role::Bottom }, suffixes }) => {
            by.0 == feature && suffixes.is_empty()
        }
        _ => false,
    }
}

/// The design's timeline order, read once for a request: each feature id by its place.
pub struct Timeline(HashMap<u64, usize>);

impl Timeline {
    pub fn of(doc: &Document) -> Timeline {
        Timeline(doc.features.iter().enumerate().map(|(at, f)| (f.id, at)).collect())
    }

    /// The feature `reference` covers, by the names it keeps, if any.
    pub fn covers(&self, reference: Reference) -> Option<u64> {
        let makers: Vec<FeatureId> = match reference {
            Reference::Edge(edge) => edge_makers(&edge.name),
            Reference::Point(point) => point.edges.iter().flat_map(|name| edge_makers(name)).collect(),
            Reference::Face(face) => face_maker(&face.name).into_iter().collect(),
        };
        let mut latest: Option<(usize, u64)> = None;
        for feature in makers {
            let at = *self.0.get(&feature.0)?;
            latest = latest.max(Some((at, feature.0)));
        }
        latest.map(|(_, feature)| feature)
    }
}

#[cfg(test)]
#[path = "coverage_tests.rs"]
mod tests;
