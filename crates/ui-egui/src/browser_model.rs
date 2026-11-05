//! Model-only browser queries, reused while navigating. Resolving a placement walks the
//! definition's bodies; doing that for every offscreen row and folder made large imports
//! quadratic on every frame. Keep the document and evaluated state alive with the index so
//! neither a new document nor a later allocation can accidentally reuse its answers.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use solidcraft_engine::Session;
use solidcraft_engine::doc::{Document, FeatureKind, Mat, ModelState};
use solidcraft_engine::occurrences::OccurrencePlacement;

pub(super) struct Model {
    doc: Arc<Document>,
    state: Arc<ModelState>,
    pending_moves: BTreeMap<u64, Mat>,
    pub bodies: HashMap<u64, Vec<usize>>,
    pub sketches: HashMap<u64, Vec<usize>>,
    pub planes: HashMap<u64, Vec<usize>>,
    pub features: HashMap<u64, Vec<usize>>,
    pub children: HashMap<u64, Vec<u64>>,
    placements: HashMap<Vec<u64>, Option<Arc<OccurrencePlacement>>>,
    /// Each placed body's placement path, by the body's world name (built on first use).
    body_paths: Option<HashMap<String, Vec<u64>>>,
}

impl Model {
    pub fn new(session: &Session) -> Self {
        let state = session.model.state();
        let mut bodies: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, body) in state.bodies.iter().enumerate() {
            bodies.entry(session.doc.model_body_component(body)).or_default().push(i);
        }
        let mut sketches: HashMap<u64, Vec<usize>> = HashMap::new();
        let mut planes: HashMap<u64, Vec<usize>> = HashMap::new();
        let mut features: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, feature) in session.doc.features.iter().enumerate() {
            if !matches!(feature.kind, FeatureKind::Sketch { .. }) {
                features.entry(feature.component).or_default().push(i);
            }
            if session.doc.marker.is_some_and(|marker| i >= marker) {
                continue;
            }
            let index = match feature.kind {
                FeatureKind::Sketch { .. } => &mut sketches,
                FeatureKind::ConstructionPlane { .. } | FeatureKind::ConstructionAxis { .. } | FeatureKind::ConstructionPoint { .. } => &mut planes,
                _ => continue,
            };
            index.entry(feature.component).or_default().push(i);
        }
        let mut children: HashMap<u64, Vec<u64>> = HashMap::new();
        for occurrence in &session.doc.occurrences {
            children.entry(occurrence.parent).or_default().push(occurrence.id);
        }
        // Sub-components in the order the design saved for them; the rest follow in placement order.
        for (parent, ids) in &mut children {
            session.doc.sort_placements(*parent, ids);
        }
        Self {
            doc: session.doc.clone(),
            state,
            pending_moves: session.pending_moves.clone(),
            bodies,
            sketches,
            planes,
            features,
            children,
            placements: HashMap::new(),
            body_paths: None,
        }
    }

    pub fn matches(&self, session: &Session) -> bool {
        Arc::ptr_eq(&self.doc, &session.doc) && Arc::ptr_eq(&self.state, &session.model.state()) && self.pending_moves == session.pending_moves
    }

    pub fn placement(&mut self, session: &Session, path: &[u64]) -> Option<Arc<OccurrencePlacement>> {
        self.placements.entry(path.to_vec()).or_insert_with(|| session.occurrence_placement(path).ok().map(Arc::new)).clone()
    }

    /// The placement a placed body (by its world name) belongs to, as the browser's occurrence
    /// rows nest; `None` for a body of the top-level design. Every placement is walked once per
    /// model, as deep and as far as the browser draws them.
    pub fn body_path(&mut self, session: &Session, body: &str) -> Option<Vec<u64>> {
        if self.body_paths.is_none() {
            let mut paths: HashMap<String, Vec<u64>> = HashMap::new();
            let mut stack: Vec<Vec<u64>> = self.children.get(&0).into_iter().flatten().map(|o| vec![*o]).collect();
            let mut walked = 0;
            while let Some(path) = stack.pop() {
                walked += 1;
                if walked > 100_000 {
                    break;
                }
                let Some(placement) = self.placement(session, &path) else { continue };
                for b in &placement.bodies {
                    paths.entry(b.world.clone()).or_insert_with(|| path.clone());
                }
                if path.len() < 64 {
                    for child in self.children.get(&placement.component).into_iter().flatten() {
                        let mut next = path.clone();
                        next.push(*child);
                        stack.push(next);
                    }
                }
            }
            self.body_paths = Some(paths);
        }
        self.body_paths.as_ref()?.get(body).cloned()
    }

    /// Whether [`Model::body_path`] has walked the placements for this model.
    #[cfg(test)]
    pub fn body_paths_walked(&self) -> bool {
        self.body_paths.is_some()
    }
}
