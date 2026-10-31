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
    child_rows: HashMap<Vec<u64>, Arc<[Child]>>,
    definitions: HashMap<u64, usize>,
    occurrence_ids: HashMap<u64, usize>,
}

pub(super) enum Child {
    Occurrence(Vec<u64>),
    Group { key: String, label: String, members: Vec<Vec<u64>> },
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
        let mut occurrence_ids = HashMap::with_capacity(session.doc.occurrences.len());
        for (i, occurrence) in session.doc.occurrences.iter().enumerate() {
            children.entry(occurrence.parent).or_default().push(occurrence.id);
            occurrence_ids.insert(occurrence.id, i);
        }
        // Sub-components in the order the design saved for them; the rest follow in placement order.
        for (parent, ids) in &mut children {
            session.doc.sort_placements(*parent, ids);
        }
        let definitions = session.doc.components.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
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
            child_rows: HashMap::new(),
            definitions,
            occurrence_ids,
        }
    }

    pub fn matches(&self, session: &Session) -> bool {
        Arc::ptr_eq(&self.doc, &session.doc) && Arc::ptr_eq(&self.state, &session.model.state()) && self.pending_moves == session.pending_moves
    }

    /// Build each expanded parent's rows once; grouping never changes document membership.
    pub fn child_rows(&mut self, component: u64, path: &[u64]) -> Arc<[Child]> {
        if let Some(rows) = self.child_rows.get(path) {
            return rows.clone();
        }
        let occurrence = |id: &u64| self.occurrence_ids.get(id).and_then(|i| self.doc.occurrences.get(*i));
        let definition = |id: &u64| self.definitions.get(id).and_then(|i| self.doc.components.get(*i));
        let children = self.children.get(&component).map_or(&[][..], Vec::as_slice);
        let mut groups: HashMap<u64, Vec<u64>> = HashMap::new();
        for id in children {
            if let Some(o) = occurrence(id)
                && definition(&o.component).is_some_and(|c| c.imported_from.is_some())
            {
                groups.entry(o.component).or_default().push(*id);
            }
        }
        let mut rows = Vec::with_capacity(children.len());
        let mut emitted = std::collections::HashSet::with_capacity(groups.len());
        let placed = |id| {
            let mut placed = Vec::with_capacity(path.len() + 1);
            placed.extend_from_slice(path);
            placed.push(id);
            placed
        };
        for id in children {
            let component = occurrence(id).map(|o| o.component);
            if component.is_some_and(|c| emitted.contains(&c)) {
                continue;
            }
            if let Some(component) = component
                && let Some(members) = groups.remove(&component)
                && members.len() > 1
            {
                let name = definition(&component).map_or("Part", |c| c.name.as_str());
                rows.push(Child::Group {
                    key: format!("{}/instances:{component}", Session::folder_scope(path)),
                    label: format!("{name} ({})", members.len()),
                    members: members.into_iter().map(&placed).collect(),
                });
                emitted.insert(component);
                continue;
            }
            rows.push(Child::Occurrence(placed(*id)));
        }
        let rows: Arc<[Child]> = rows.into();
        self.child_rows.insert(path.to_vec(), rows.clone());
        rows
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
}
