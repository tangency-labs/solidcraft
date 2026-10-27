//! Exact placement identity and session view/authoring state.
use crate::{EngineError, Result, Sel, Session};
use solidcraft_doc::{FeatureKind, IDENTITY, Mat, PlaneRef, mat_mul};

#[derive(Clone, Debug)]
pub struct OccurrenceBody {
    pub source: String,
    pub world: String,
}

#[derive(Clone, Debug)]
pub struct OccurrencePlacement {
    pub occurrence: u64,
    pub component: u64,
    pub path: Vec<u64>,
    pub name: String,
    pub world: Mat,
    pub local: Mat,
    pub parent_world: Mat,
    pub grounded: bool,
    pub bodies: Vec<OccurrenceBody>,
}

fn invalid(message: &str) -> EngineError {
    EngineError::Other(message.to_string())
}

pub(crate) struct VisibilityCache {
    revision: u64,
    paths: Vec<Vec<u64>>,
    folders: Vec<String>,
    bodies: std::collections::BTreeSet<String>,
}

/// [`Session::occurrence_bodies`] answers for one document, evaluated model and set of pending
/// moves. The document and model are kept alive with the answers, so neither a new document nor
/// a later allocation at the same address can reuse them.
pub(crate) struct OccurrenceBodiesCache {
    doc: std::sync::Arc<solidcraft_doc::Document>,
    model: std::sync::Arc<solidcraft_doc::ModelState>,
    pending_moves: std::collections::BTreeMap<u64, Mat>,
    bodies: std::collections::HashMap<(Vec<u64>, bool), std::sync::Arc<[String]>>,
}

/// How many placements' answers [`OccurrenceBodiesCache`] keeps before it starts again.
const OCCURRENCE_BODIES_CACHED: usize = 256;

impl OccurrenceBodiesCache {
    fn matches(&self, session: &Session, model: &std::sync::Arc<solidcraft_doc::ModelState>) -> bool {
        std::sync::Arc::ptr_eq(&self.doc, &session.doc) && std::sync::Arc::ptr_eq(&self.model, model) && self.pending_moves == session.pending_moves
    }
}

/// [`Session::body_removed_later`] for one revision, sketch and history marker.
pub(crate) struct RemovedLaterCache {
    revision: u64,
    sketch: u64,
    marker: usize,
    bodies: std::collections::BTreeSet<String>,
}

impl Session {
    /// Hidden names follow the same body and exact occurrence path through a rename. Undo
    /// history restores visibility separately. Never parse display labels:
    /// ambiguous labels and names containing slashes use the document's canonical naming.
    pub(crate) fn remap_hidden_bodies(
        &mut self,
        previous: &solidcraft_doc::Document,
        state: &solidcraft_doc::ModelState,
        rename: Option<(&str, &str)>,
    ) {
        if self.visibility.hidden_bodies.is_empty() {
            return;
        }
        if rename.is_none() {
            if previous.occurrences == self.doc.occurrences {
                return;
            }
            let old_names: std::collections::BTreeMap<_, _> = previous.occurrences.iter().map(|o| (o.id, &o.name)).collect();
            if !self.doc.occurrences.iter().any(|o| old_names.get(&o.id).is_some_and(|name| **name != o.name)) {
                return;
            }
        }
        let mut renamed = state.clone();
        if let Some((old, new)) = rename {
            for body in renamed.bodies.iter_mut().filter(|body| body.name == old) {
                body.name = new.to_string();
            }
        }
        let hidden: std::collections::BTreeSet<_> = self.visibility.hidden_bodies.iter().map(String::as_str).collect();
        let mut names = std::collections::BTreeMap::new();
        let (before, after) = (solidcraft_doc::WorldNames::new(previous, state), solidcraft_doc::WorldNames::new(&self.doc, &renamed));
        for (component, path, _) in previous.placements() {
            for body in state.bodies.iter().filter(|body| previous.model_body_component(body) == component) {
                let old = before.name(&body.name, component, &path);
                if hidden.contains(old.as_str()) {
                    let source = rename.filter(|(old, _)| *old == body.name).map_or(body.name.as_str(), |(_, new)| new);
                    names.insert(old, after.name(source, component, &path));
                }
            }
        }
        for hidden in &mut self.visibility.hidden_bodies {
            if let Some(new) = names.get(hidden) {
                hidden.clone_from(new);
            }
        }
    }

    /// Visibility keeps construction names, while undo history keeps feature ids. Follow the
    /// same axis or point into the new document on rename, undo and redo (#4419).
    pub(crate) fn follow_axis_point_renames(&mut self, previous: &solidcraft_doc::Document) {
        if self.visibility.hidden_origin.is_empty() {
            return;
        }
        let names: std::collections::HashMap<_, _> = self.doc.features.iter().map(|f| (f.id, &f.name)).collect();
        let renamed: std::collections::HashMap<_, _> = previous
            .features
            .iter()
            .filter(|f| matches!(f.kind, FeatureKind::ConstructionAxis { .. } | FeatureKind::ConstructionPoint { .. }))
            .filter_map(|f| names.get(&f.id).filter(|name| **name != &f.name).map(|name| (f.name.as_str(), *name)))
            .collect();
        for name in &mut self.visibility.hidden_origin {
            if let Some(new) = renamed.get(name.as_str()) {
                name.clone_from(new);
            }
        }
    }

    /// Document with uncaptured occurrence positions applied, without changing saved geometry.
    pub fn placed_document(&self) -> std::borrow::Cow<'_, solidcraft_doc::Document> {
        if self.pending_moves.is_empty() {
            return std::borrow::Cow::Borrowed(&self.doc);
        }
        let mut doc = (*self.doc).clone();
        for o in &mut doc.occurrences {
            if let Some(transform) = self.pending_moves.get(&o.id) {
                o.transform = *transform;
            }
        }
        std::borrow::Cow::Owned(doc)
    }
    /// Resolve only the supplied path, never an exponentially expanded assembly.
    pub fn occurrence_placement(&self, path: &[u64]) -> Result<OccurrencePlacement> {
        if path.is_empty() || path.len() > 64 {
            return Err(invalid("occurrence path must contain 1..64 ids"));
        }
        let mut component = 0;
        let mut world = IDENTITY;
        let mut parent_world = IDENTITY;
        let mut seen = std::collections::BTreeSet::new();
        for id in path {
            if !seen.insert(*id) {
                return Err(invalid("occurrence path repeats an id"));
            }
            let o = self
                .doc
                .occurrences
                .iter()
                .find(|o| o.id == *id && o.parent == component)
                .ok_or_else(|| invalid("occurrence path has a missing or mismatched parent"))?;
            if !self.doc.components.iter().any(|c| c.id == o.component) {
                return Err(invalid("occurrence path has a missing component"));
            }
            parent_world = world;
            world = mat_mul(&world, self.pending_moves.get(id).unwrap_or(&o.transform));
            if world.iter().flatten().any(|x| !x.is_finite()) {
                return Err(invalid("occurrence path has a non-finite transform"));
            }
            component = o.component;
        }
        let leaf = self.doc.occurrences.iter().find(|o| Some(&o.id) == path.last()).ok_or_else(|| invalid("occurrence path has no leaf"))?;
        let state = self.model.state();
        let names = solidcraft_doc::WorldNames::new(&self.doc, &state);
        let bodies = state
            .bodies
            .iter()
            .filter(|b| self.doc.model_body_component(b) == component)
            .map(|b| OccurrenceBody { source: b.name.clone(), world: names.name(&b.name, component, path) })
            .collect();
        Ok(OccurrencePlacement {
            occurrence: leaf.id,
            component,
            path: path.to_vec(),
            name: leaf.name.clone(),
            world,
            local: self.pending_moves.get(&leaf.id).copied().unwrap_or(leaf.transform),
            parent_world,
            grounded: leaf.grounded,
            bodies,
        })
    }

    /// Children are definitions shared by every placement of the parent component.
    pub fn occurrence_children(&self, path: &[u64]) -> Result<Vec<OccurrencePlacement>> {
        let component = if path.is_empty() { 0 } else { self.occurrence_placement(path)?.component };
        self.doc
            .occurrences
            .iter()
            .filter(|o| o.parent == component)
            .map(|o| {
                let mut child = path.to_vec();
                child.push(o.id);
                self.occurrence_placement(&child)
            })
            .collect()
    }

    /// Bounded descendant expansion. Never return an incomplete selection or bounds list.
    pub fn occurrence_bodies(&self, path: &[u64], descendants: bool) -> Result<Vec<String>> {
        Ok(self.occurrence_bodies_shared(path, descendants)?.to_vec())
    }

    /// [`Self::occurrence_bodies`], shared and remembered until the document, the evaluated
    /// model or the pending moves change: a nested placement's subtree is walked once, not on
    /// every frame that lights it (a browser row under the pointer, the active placement; #3892).
    pub fn occurrence_bodies_shared(&self, path: &[u64], descendants: bool) -> Result<std::sync::Arc<[String]>> {
        let model = self.model.state();
        let key = (path.to_vec(), descendants);
        if let Ok(cache) = self.occurrence_bodies_cache.lock()
            && let Some(bodies) = cache.as_ref().filter(|c| c.matches(self, &model)).and_then(|c| c.bodies.get(&key))
        {
            return Ok(bodies.clone());
        }
        let bodies: std::sync::Arc<[String]> = self.expand_occurrence_bodies(path, descendants)?.into();
        if let Ok(mut cache) = self.occurrence_bodies_cache.lock() {
            if cache.as_ref().is_none_or(|c| !c.matches(self, &model) || c.bodies.len() >= OCCURRENCE_BODIES_CACHED) {
                *cache = Some(OccurrenceBodiesCache {
                    doc: self.doc.clone(),
                    model,
                    pending_moves: self.pending_moves.clone(),
                    bodies: std::collections::HashMap::new(),
                });
            }
            if let Some(cache) = cache.as_mut() {
                cache.bodies.insert(key, bodies.clone());
            }
        }
        Ok(bodies)
    }

    fn expand_occurrence_bodies(&self, path: &[u64], descendants: bool) -> Result<Vec<String>> {
        let mut stack = vec![self.occurrence_placement(path)?];
        let mut out = Vec::new();
        let mut count = 0;
        while let Some(placement) = stack.pop() {
            count += 1;
            if count > 100_000 || out.len() + placement.bodies.len() > 100_000 {
                return Err(invalid("occurrence expansion exceeds 100000 placements or bodies"));
            }
            out.extend(placement.bodies.into_iter().map(|b| b.world));
            if descendants {
                stack.extend(self.occurrence_children(&placement.path)?);
            }
        }
        Ok(out)
    }

    /// The world names of the bodies `features` made (#5099), in every placement of their
    /// component, or only in `placement` (a component and the path of one of its placements).
    pub fn feature_bodies(&self, features: &[u64], placement: Option<(u64, &[u64])>) -> Vec<String> {
        let state = self.model.state();
        let made: Vec<_> = state.bodies.iter().filter(|b| features.contains(&b.feature)).collect();
        if made.is_empty() {
            return Vec::new();
        }
        let placements = match placement {
            Some((component, path)) => vec![(component, path.to_vec())],
            None => self.doc.placements().into_iter().map(|(component, path, _)| (component, path)).collect(),
        };
        let mut out = Vec::new();
        for (component, path) in &placements {
            for b in made.iter().filter(|b| self.doc.model_body_component(b) == *component) {
                let name = self.doc.world_body_name(&state, &b.name, *component, path);
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
        out
    }

    /// Selected authoring placement, or the compatible first-placement frame.
    pub fn active_component_frame(&self) -> Mat {
        if let Some(path) = &self.active_occurrence_path
            && let Ok(p) = self.occurrence_placement(path)
            && p.component == self.active_component
        {
            return p.world;
        }
        let path = self.doc.first_path(self.active_component);
        self.occurrence_placement(&path).map(|p| p.world).unwrap_or(IDENTITY)
    }

    pub fn occurrence_visible(&self, path: &[u64]) -> bool {
        !self.visibility.hidden_occurrences.iter().any(|hidden| path.starts_with(hidden))
    }

    pub fn occurrence_hidden(&self, path: &[u64]) -> bool {
        self.visibility.hidden_occurrences.iter().any(|hidden| hidden == path)
    }

    /// Number of placements affected by editing this shared occurrence definition.
    pub fn occurrence_scope_count(&self, path: &[u64]) -> Result<usize> {
        let id = self.occurrence_placement(path)?.occurrence;
        self.occurrence_id_scope_count(id)
    }

    pub fn occurrence_id_scope_count(&self, id: u64) -> Result<usize> {
        if !self.doc.occurrences.iter().any(|o| o.id == id) {
            return Err(invalid("no occurrence"));
        }
        let mut stack = vec![(0, 0usize)];
        let mut visited = 0;
        let mut count = 0;
        while let Some((component, depth)) = stack.pop() {
            for o in self.doc.occurrences.iter().filter(|o| o.parent == component) {
                visited += 1;
                if depth >= 64 || visited > 100_000 {
                    return Err(invalid("occurrence scope exceeds 64 levels or 100000 placements"));
                }
                if o.id == id {
                    count += 1;
                }
                stack.push((o.component, depth + 1));
            }
        }
        Ok(count)
    }

    /// Whether a joint's glyph is drawn (`browser.visibility`). Hiding a joint only hides its
    /// drawing: it still constrains and drives the parts.
    /// Is the joint shown itself (its own eye; the Joints folder may still hide it)?
    pub fn joint_visible(&self, id: u64) -> bool {
        !self.visibility.hidden_joints.contains(&id)
    }

    /// Is the joint's glyph drawn: shown itself, in a shown Joints folder?
    pub fn joint_drawn(&self, id: u64) -> bool {
        self.joint_visible(id) && !self.folder_hidden("c0/joints")
    }

    /// The scope of a placement's browser folders and groups: `c0` for the design (an empty
    /// path), `o:<ids joined by />` for a placement.
    pub fn folder_scope(path: &[u64]) -> String {
        if path.is_empty() { "c0".to_string() } else { format!("o:{}", path.iter().map(u64::to_string).collect::<Vec<_>>().join("/")) }
    }

    /// Is a browser folder or group hidden itself (by tree key, see `Visibility::hidden_folders`)?
    pub fn folder_hidden(&self, key: &str) -> bool {
        self.visibility.hidden_folders.iter().any(|k| k == key)
    }

    /// Is an item (its browser key: body name, feature id) hidden by its folder, or by a group
    /// of that folder it is in, at a placement scope ([`Self::folder_scope`])?
    pub fn item_folder_hidden(&self, scope: &str, component: u64, folder: &str, item: &str) -> bool {
        if self.visibility.hidden_folders.is_empty() {
            return false;
        }
        self.folder_hidden(&format!("{scope}/{folder}"))
            || self.doc.browser_groups.iter().any(|g| {
                g.component == component
                    && g.folder == folder
                    && g.items.iter().any(|i| i == item)
                    && self.folder_hidden(&format!("{scope}/g{}", g.id))
            })
    }

    /// Is a sketch or construction feature hidden by its folder or group, at the placement it
    /// is drawn at (the active one while its component is being edited, else the first)?
    pub fn feature_folder_hidden(&self, id: u64, folder: &str) -> bool {
        if self.visibility.hidden_folders.is_empty() {
            return false;
        }
        let component = self.doc.feature(id).map_or(0, |f| f.component);
        let path = match &self.active_occurrence_path {
            _ if component == 0 => Vec::new(),
            Some(p) if self.active_component == component => p.clone(),
            _ => self.doc.first_path(component),
        };
        self.item_folder_hidden(&Self::folder_scope(&path), component, folder, &id.to_string())
    }

    /// Is construction geometry or an origin item (by name) shown: not hidden itself, nor by
    /// its folder or a group it is in?
    pub fn construction_visible(&self, name: &str) -> bool {
        if self.visibility.hidden_origin.iter().any(|h| h == name) {
            return false;
        }
        match self.doc.features.iter().find(|f| f.name == name) {
            Some(f) => !self.feature_folder_hidden(f.id, "construction"),
            None => true,
        }
    }

    pub fn body_visible(&self, name: &str) -> bool {
        if self.visibility.hidden_bodies.iter().any(|n| n == name) || self.body_removed_later(name) {
            return false;
        }
        let v = &self.visibility;
        if v.hidden_occurrences.is_empty() && v.hidden_folders.is_empty() {
            return true;
        }
        if let Ok(mut cache) = self.occurrence_visibility_cache.lock() {
            if cache.as_ref().is_none_or(|c| c.revision != self.revision || c.paths != v.hidden_occurrences || c.folders != v.hidden_folders) {
                *cache = Some(VisibilityCache {
                    revision: self.revision,
                    paths: v.hidden_occurrences.clone(),
                    folders: v.hidden_folders.clone(),
                    bodies: self.bodies_hidden_by_parents(),
                });
            }
            return cache.as_ref().is_none_or(|c| !c.bodies.contains(name));
        }
        !self.bodies_hidden_by_parents().contains(name)
    }

    /// World names of the bodies a hidden placement, folder or group above them hides (their
    /// own state aside).
    fn bodies_hidden_by_parents(&self) -> std::collections::BTreeSet<String> {
        let st = self.model.state();
        let mut out = std::collections::BTreeSet::new();
        let names = solidcraft_doc::WorldNames::new(&self.doc, &st);
        for (component, path, _) in self.doc.placements() {
            let placement_hidden = !path.is_empty() && !self.occurrence_visible(&path);
            let scope = Self::folder_scope(&path);
            for b in st.bodies.iter().filter(|b| self.doc.model_body_component(b) == component) {
                if placement_hidden || self.item_folder_hidden(&scope, component, "bodies", &b.name) {
                    out.insert(names.name(&b.name, component, &path));
                }
            }
        }
        out
    }

    /// Is this body deleted by a feature the history marker has rolled off, while a sketch is
    /// being edited? Edit Sketch rolls the timeline back to the sketch, so a body made before the
    /// sketch and removed after it is part of the shown model again; it stays out of the view,
    /// as it does once the timeline runs to the end. Nothing else changes: the model, its state
    /// and every command still see the history as it was rolled to. A body the sketch is
    /// built on (the face it sits on, an edge it projects) stays.
    pub fn body_removed_later(&self, name: &str) -> bool {
        let (Some(sketch), Some(marker)) = (self.active_sketch, self.doc.marker) else { return false };
        if marker >= self.doc.features.len() {
            return false;
        }
        let Ok(mut cache) = self.removed_later_cache.lock() else { return false };
        if cache.as_ref().is_none_or(|c| c.revision != self.revision || c.sketch != sketch || c.marker != marker) {
            *cache = Some(RemovedLaterCache { revision: self.revision, sketch, marker, bodies: self.removed_later_bodies(sketch, marker) });
        }
        cache.as_ref().is_some_and(|c| c.bodies.contains(name))
    }

    /// The names (as shown in the world) of the bodies that features from `marker` on remove,
    /// leaving out the bodies `sketch` is built on.
    fn removed_later_bodies(&self, sketch: u64, marker: usize) -> std::collections::BTreeSet<String> {
        let none = std::collections::BTreeSet::new();
        let Some(FeatureKind::Sketch { plane, sketch: made }) = self.doc.feature(sketch).map(|f| &f.kind) else { return none };
        let mut kept: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        let mut plane = plane;
        loop {
            match plane {
                PlaneRef::Offset { base, .. }
                | PlaneRef::ThroughPoint { base, .. }
                | PlaneRef::AtAngle { base, .. }
                | PlaneRef::Flipped { base, .. } => plane = base,
                // A face attachment that names no exact source: which body it sits on is unknown,
                // so none is taken from the view.
                PlaneRef::Face { anchor: None, .. } => return none,
                PlaneRef::Face { anchor: Some(anchor), .. } => {
                    kept.insert(anchor.target.body.as_str());
                    break;
                }
                _ => break,
            }
        }
        for link in &made.links {
            kept.extend(link.source.bodies());
        }
        let shown = self.model.state();
        let mut removed = std::collections::BTreeSet::new();
        for feature in self.doc.features.iter().skip(marker).filter(|f| !f.suppressed) {
            if let FeatureKind::Remove { bodies } = &feature.kind {
                removed.extend(bodies.iter().filter(|b| !kept.contains(b.0.as_str()) && shown.body(&b.0).is_some()).map(|b| b.0.clone()));
            }
        }
        if removed.is_empty() || self.doc.occurrences.is_empty() {
            return removed;
        }
        // A component's bodies are shown once per placement, each under its own name.
        self.doc
            .body_frames_with_source(&shown)
            .into_iter()
            .filter(|(_, source, _, _, _)| removed.contains(source))
            .map(|(world, ..)| world)
            .collect()
    }

    pub(crate) fn prune_occurrence_state(&mut self) {
        self.pending_moves.retain(|id, _| self.doc.occurrences.iter().any(|o| o.id == *id));
        if self.active_component != 0 && !self.doc.components.iter().any(|c| c.id == self.active_component) {
            self.active_component = 0;
            self.active_occurrence_path = None;
        }
        if self.active_occurrence_path.as_ref().is_some_and(|p| self.occurrence_placement(p).map_or(true, |p| p.component != self.active_component)) {
            self.active_occurrence_path = None;
        }
        let selection = std::mem::take(&mut self.selection);
        self.selection = selection.into_iter().filter(|s| !matches!(s, Sel::Occurrence {path} if self.occurrence_placement(path).is_err())).collect();
        let hidden = std::mem::take(&mut self.visibility.hidden_occurrences);
        self.visibility.hidden_occurrences = hidden.into_iter().filter(|p| self.occurrence_placement(p).is_ok()).collect();
    }
}
