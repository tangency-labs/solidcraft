//! Model-only browser queries, reused while navigating. Resolving a placement walks the
//! definition's bodies; doing that for every offscreen row and folder made large imports
//! quadratic on every frame. Keep the document and evaluated state alive with the index so
//! neither a new document nor a later allocation can accidentally reuse its answers. Feature rows
//! find their place, their result and the bodies they made here too; a result itself is read
//! from the session each time, as it can change in place.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

use solidcraft_engine::Session;
use solidcraft_engine::doc::{Document, FeatureKind, FeatureResult, Mat, ModelState};
use solidcraft_engine::occurrences::OccurrencePlacement;

use crate::SolveApp;
use crate::ui_kit::Status;

#[cfg(test)]
thread_local! {
    /// How often a model looked through the design's history for [`Places`] (the tests count it).
    pub(super) static PLACES_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

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
    /// The features that built each body, by its model name (worked out on first use, #11945).
    body_features: Option<BTreeMap<String, Vec<u64>>>,
    /// Where each feature and its result are (built on first use, #14850).
    places: OnceLock<Places>,
    /// The bodies each feature made, by their place in the model's bodies (built on first use).
    made: Option<HashMap<u64, Vec<usize>>>,
    /// The world names of the bodies a feature made ([`Model::feature_bodies`]), by the
    /// placement's path, then by its component (`None`: every placement) and the feature. Only the
    /// rows drawn ask, so only theirs are worked out.
    names: HashMap<Vec<u64>, HashMap<(Option<u64>, u64), Vec<String>>>,
}

/// Where each feature is in the history and where its result is: a feature row looked through
/// the whole history twice, every frame, for its mark (#14850).
struct Places {
    /// Each feature's place in the history, by id: the first, as `Document::feature_index` finds.
    by_id: HashMap<u64, usize>,
    /// The places of each component's features, sketches too, in history order.
    by_component: HashMap<u64, Vec<usize>>,
    /// Where each feature's result was in the session's results, by the feature's place. Only a
    /// hint, checked against the id when read: a result can change in place, without a new model
    /// (a motion trace's warning), and its mark still shows.
    results: Vec<Option<usize>>,
}

impl Places {
    fn new(doc: &Document, results: &[FeatureResult]) -> Self {
        #[cfg(test)]
        PLACES_BUILT.with(|n| n.set(n.get() + 1));
        let found = crate::timeline::result_index(results);
        let mut places = Places { by_id: HashMap::with_capacity(doc.features.len()), by_component: HashMap::new(), results: Vec::new() };
        for (i, feature) in doc.features.iter().enumerate() {
            places.by_id.entry(feature.id).or_insert(i);
            places.by_component.entry(feature.component).or_default().push(i);
            places.results.push(found.get(&feature.id).copied());
        }
        places
    }
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
            body_features: None,
            places: OnceLock::new(),
            made: None,
            names: HashMap::new(),
        }
    }

    /// Where the design's features and their results are, looked up once per model.
    fn places(&self, session: &Session) -> &Places {
        self.places.get_or_init(|| Places::new(&self.doc, &session.model.results))
    }

    /// A feature's place in the history ([`Document::feature_index`]).
    pub fn position(&self, session: &Session, id: u64) -> Option<usize> {
        self.places(session).by_id.get(&id).copied()
    }

    /// The places of a component's features, sketches too, in history order: the rows of its
    /// History in the Browser.
    pub fn history_features(&self, session: &Session, component: u64) -> &[usize] {
        self.places(session).by_component.get(&component).map_or(&[], Vec::as_slice)
    }

    /// The result of the feature at `index` in the history, as `session` has it now.
    fn result_at<'s>(&self, session: &'s Session, index: usize) -> Option<&'s FeatureResult> {
        let id = self.doc.features.get(index)?.id;
        let results = &session.model.results;
        match self.places(session).results.get(index).copied().flatten() {
            Some(k) if results.get(k).is_some_and(|result| result.id == id) => results.get(k),
            _ => session.model.result(id),
        }
    }

    /// A feature's mark ([`crate::timeline::feature_status`]), from the results as they are now.
    pub fn status(&self, app: &SolveApp, id: u64) -> Option<Status> {
        let index = self.position(&app.session, id)?;
        crate::timeline::status_at(app, index, || self.result_at(&app.session, index))
    }

    /// The worst mark of a component's features (its sketches too in the History layout) and the
    /// first feature with it ([`crate::timeline::worst_status`]), from the results as they are
    /// now. Only the folder's own features are read: a folded folder scanned the whole history,
    /// every frame (#6083).
    pub fn folder_status(&self, app: &SolveApp, component: u64, history: bool) -> Option<(Status, u64)> {
        let features =
            if history { self.history_features(&app.session, component) } else { self.features.get(&component).map_or(&[][..], Vec::as_slice) };
        let mut worst: Option<(Status, u64)> = None;
        for &index in features {
            let Some(status) = crate::timeline::status_at(app, index, || self.result_at(&app.session, index)) else { continue };
            if worst.is_none_or(|(mark, _)| status > mark) {
                worst = self.doc.features.get(index).map(|feature| (status, feature.id));
            }
        }
        worst
    }

    /// The world names of the bodies feature `id` made ([`Session::feature_bodies`]), in
    /// `placement` (a component and the path of one of its placements; `None`: every placement).
    /// Worked out once per model for each row drawn: the session's own answer looks through every
    /// body of the design, and every row asked it every frame (#14850).
    pub fn feature_bodies(&mut self, id: u64, placement: Option<(u64, &[u64])>) -> &[String] {
        let (component, path) = placement.map_or((None, &[][..]), |(component, path)| (Some(component), path));
        let key = (component, id);
        if !self.names.get(path).is_some_and(|names| names.contains_key(&key)) {
            let names = self.made_names(id, placement);
            self.names.entry(path.to_vec()).or_default().insert(key, names);
        }
        self.names.get(path).and_then(|names| names.get(&key)).map_or(&[], Vec::as_slice)
    }

    /// [`Session::feature_bodies`] for one feature, from the bodies it made rather than from every
    /// body. The model's design and state are the session's ([`Model::matches`]).
    fn made_names(&mut self, id: u64, placement: Option<(u64, &[u64])>) -> Vec<String> {
        let state = &self.state;
        let made = self.made.get_or_insert_with(|| {
            let mut made: HashMap<u64, Vec<usize>> = HashMap::new();
            for (i, body) in state.bodies.iter().enumerate() {
                made.entry(body.feature).or_default().push(i);
            }
            made
        });
        let bodies: Vec<_> = made.get(&id).into_iter().flatten().filter_map(|i| state.bodies.get(*i)).collect();
        if bodies.is_empty() {
            return Vec::new();
        }
        let placements = match placement {
            Some((component, path)) => vec![(component, path.to_vec())],
            None => self.doc.placements().into_iter().map(|(component, path, _)| (component, path)).collect(),
        };
        let mut out = Vec::new();
        for (component, path) in &placements {
            for body in bodies.iter().filter(|body| self.doc.model_body_component(body) == *component) {
                let name = self.doc.world_body_name(state, &body.name, *component, path);
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
        out
    }

    /// The features that built the body with this model name, in timeline order
    /// ([`Session::body_features`]), worked out once per model.
    pub fn body_features(&mut self, session: &Session, body: &str) -> Vec<u64> {
        self.body_features.get_or_insert_with(|| session.body_features()).get(body).cloned().unwrap_or_default()
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
