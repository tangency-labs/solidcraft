//! Exact placement identity and session view/authoring state.
use crate::{EngineError, Result, Sel, Session, Visibility};
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

/// The hidden joints, folders and bodies of the view, as sets: the browser asks about every
/// joint and every folder of the design on every frame, the viewport about every body several
/// times a frame, and an answer that walked the hidden lists made a frame cost grow with the
/// size of the design (#3892). The sets are built when the view changes and are kept until it
/// changes again.
pub(crate) struct VisibilityIndex {
    /// The revision of the design the sets were built for.
    revision: u64,
    /// Where the hidden joints were when the sets were built: the list is replaced (the
    /// whole view is replaced) or written in place (a joint is hidden or shown), and this
    /// says whether the list the sets were built from is still the list of the view.
    joints_at: ListId,
    /// The hidden joint ids.
    joint_ids: std::collections::HashSet<u64>,
    /// Where the hidden folders were when the sets were built.
    folders_at: ListId,
    /// The hidden folder keys.
    folder_keys: std::collections::HashSet<String>,
    /// Where the hidden bodies were when the sets were built.
    bodies_at: ListId,
    /// The names of the bodies hidden themselves.
    body_names: std::collections::HashSet<String>,
    /// The hidden joints and folders, as they were when the sets were built: they are
    /// compared with these to tell a change of the view from a change of the design alone
    /// (the design changes far more often than the view does).
    joints: Vec<u64>,
    folders: Vec<String>,
    bodies: Vec<String>,
}

/// Where a list of hidden items was: the address of its buffer and how many items it held.
/// A list that is replaced gets a buffer of its own (the old one is still alive while the
/// new one is allocated, so the addresses differ), and a list that is written in place keeps
/// its buffer; the length then says how much of it is written.
#[derive(Clone, Copy, PartialEq, Eq)]
struct ListId {
    address: usize,
    length: usize,
}

impl ListId {
    fn of<T>(list: &[T]) -> ListId {
        ListId { address: list.as_ptr() as usize, length: list.len() }
    }
}

impl VisibilityIndex {
    /// The sets of the hidden joints and folders of a view.
    fn new(revision: u64, v: &Visibility) -> VisibilityIndex {
        let joint_ids: std::collections::HashSet<u64> = v.hidden_joints.iter().copied().collect();
        let folder_keys: std::collections::HashSet<String> = v.hidden_folders.iter().cloned().collect();
        let body_names: std::collections::HashSet<String> = v.hidden_bodies.iter().cloned().collect();
        visibility_work::built(joint_ids.len() as u64 + folder_keys.len() as u64 + body_names.len() as u64);
        VisibilityIndex {
            revision,
            joints_at: ListId::of(&v.hidden_joints),
            joint_ids,
            folders_at: ListId::of(&v.hidden_folders),
            folder_keys,
            bodies_at: ListId::of(&v.hidden_bodies),
            body_names,
            joints: v.hidden_joints.clone(),
            folders: v.hidden_folders.clone(),
            bodies: v.hidden_bodies.clone(),
        }
    }

    /// Do the sets still answer for this design and this view? The revision says the design
    /// changed; the lists say the view changed under it (the view is replaced outside the
    /// engine too, without a revision of its own).
    ///
    /// The first and last entries of each list are part of the answer: a buffer that was freed
    /// and handed out again holds another list, and the entries say so.
    fn matches(&self, revision: u64, v: &Visibility) -> bool {
        self.revision == revision
            && self.joints_at == ListId::of(&v.hidden_joints)
            && self.folders_at == ListId::of(&v.hidden_folders)
            && self.joints.first() == v.hidden_joints.first()
            && self.joints.last() == v.hidden_joints.last()
            && self.folders.first() == v.hidden_folders.first()
            && self.folders.last() == v.hidden_folders.last()
            && self.bodies_at == ListId::of(&v.hidden_bodies)
            && self.bodies.first() == v.hidden_bodies.first()
            && self.bodies.last() == v.hidden_bodies.last()
    }

    /// Are the hidden joints and folders the ones the sets were built from? A design that
    /// changed without touching the view leaves the sets alone.
    fn same_lists(&self, v: &Visibility) -> bool {
        self.joints == v.hidden_joints && self.folders == v.hidden_folders && self.bodies == v.hidden_bodies
    }

    /// The lists are the ones the sets were built from, at another revision.
    fn restamp(&mut self, revision: u64, v: &Visibility) {
        self.revision = revision;
        self.joints_at = ListId::of(&v.hidden_joints);
        self.folders_at = ListId::of(&v.hidden_folders);
        self.bodies_at = ListId::of(&v.hidden_bodies);
    }
}

/// How many entries of the hidden lists a visibility answer had to walk, and how many the
/// index was built from. The browser asks about every joint of the design on every frame, so
/// the answer must not cost more as the design grows (#3892); the counters make that cost
/// visible to a test.
pub(crate) mod visibility_work {
    #[cfg(test)]
    use std::cell::Cell;

    // The counter of the thread that asks: the tests run beside each other, and each counts
    // its own work.
    #[cfg(test)]
    thread_local! {
        static WALKED: Cell<u64> = const { Cell::new(0) };
        static BUILT: Cell<u64> = const { Cell::new(0) };
    }

    /// The entries the answers of this thread walked since the last [`take`].
    #[cfg(test)]
    pub(crate) fn take() -> u64 {
        WALKED.with(|walked| walked.replace(0))
    }

    /// Count the entries an answer walked.
    pub(crate) fn add(walked: u64) {
        #[cfg(test)]
        WALKED.with(|count| count.set(count.get() + walked));
        #[cfg(not(test))]
        let _ = walked;
    }

    /// Count the entries the index was built from: the index is built once per change of the
    /// view, not once per answer, so this work is apart from the answers' own.
    pub(crate) fn built(built: u64) {
        #[cfg(test)]
        BUILT.with(|count| count.set(count.get() + built));
        #[cfg(not(test))]
        let _ = built;
    }

    /// The entries the index was built from since the last [`built_take`].
    #[cfg(test)]
    pub(crate) fn built_take() -> u64 {
        BUILT.with(|count| count.replace(0))
    }
}

/// [`Session::occurrence_bodies`] answers for one document, evaluated model and set of pending
/// moves. The document and model are kept alive with the answers, so neither a new document nor
/// a later allocation at the same address can reuse them.
pub(crate) struct OccurrenceBodiesCache {
    doc: std::sync::Arc<solidcraft_doc::Document>,
    model: std::sync::Arc<solidcraft_doc::ModelState>,
    pending_moves: std::collections::BTreeMap<u64, Mat>,
    pub(crate) bodies: std::collections::HashMap<(Vec<u64>, bool), std::sync::Arc<[String]>>,
}

/// How many placements' answers [`OccurrenceBodiesCache`] keeps before retiring one for each
/// new one.
const OCCURRENCE_BODIES_CACHED: usize = 256;

/// Placement identity for one document and evaluated model, worked out once per design:
/// occurrences by (parent, id), children per parent, bodies per component, and each placement
/// path already named. Listing a subtree then costs the subtree, not the subtree times the
/// design (#7502). Pending moves change a placement's matrices but never its component or its
/// bodies' names, so the index is keyed on the document and model alone.
pub(crate) struct PlacementIndex {
    doc: std::sync::Arc<solidcraft_doc::Document>,
    state: std::sync::Arc<solidcraft_doc::ModelState>,
    /// The index into `doc.occurrences` of each (parent component, occurrence id).
    by_parent_id: std::collections::HashMap<(u64, u64), usize>,
    /// The occurrence ids of each parent component's children, in document order.
    children: std::collections::HashMap<u64, Vec<u64>>,
    /// The source body names of each component.
    bodies: std::collections::HashMap<u64, Vec<String>>,
    /// The ids of the design's components.
    components: std::collections::HashSet<u64>,
    /// Each placement path already named: its bodies' world names.
    placed: std::collections::HashMap<Vec<u64>, std::sync::Arc<[OccurrenceBody]>>,
}

/// How many placement paths [`PlacementIndex`] names before it stops remembering more.
const PLACEMENT_PATHS_CACHED: usize = solidcraft_doc::MAX_PLACEMENTS;

impl PlacementIndex {
    fn new(doc: &std::sync::Arc<solidcraft_doc::Document>, state: &std::sync::Arc<solidcraft_doc::ModelState>) -> PlacementIndex {
        let mut by_parent_id = std::collections::HashMap::with_capacity(doc.occurrences.len());
        let mut children: std::collections::HashMap<u64, Vec<u64>> = std::collections::HashMap::new();
        for (i, o) in doc.occurrences.iter().enumerate() {
            by_parent_id.insert((o.parent, o.id), i);
            children.entry(o.parent).or_default().push(o.id);
        }
        let mut bodies: std::collections::HashMap<u64, Vec<String>> = std::collections::HashMap::new();
        for b in &state.bodies {
            bodies.entry(doc.model_body_component(b)).or_default().push(b.name.clone());
        }
        PlacementIndex {
            doc: doc.clone(),
            state: state.clone(),
            by_parent_id,
            children,
            bodies,
            components: doc.components.iter().map(|c| c.id).collect(),
            placed: std::collections::HashMap::new(),
        }
    }

    fn matches(&self, session: &Session, model: &std::sync::Arc<solidcraft_doc::ModelState>) -> bool {
        std::sync::Arc::ptr_eq(&self.doc, &session.doc) && std::sync::Arc::ptr_eq(&self.state, model)
    }

    /// The component's bodies named at `path`, remembered until the index is replaced.
    fn bodies_at(&mut self, path: &[u64], component: u64) -> std::sync::Arc<[OccurrenceBody]> {
        if let Some(bodies) = self.placed.get(path) {
            return bodies.clone();
        }
        let names = solidcraft_doc::WorldNames::new(&self.doc, &self.state);
        let empty = Vec::new();
        let bodies: std::sync::Arc<[OccurrenceBody]> = self
            .bodies
            .get(&component)
            .unwrap_or(&empty)
            .iter()
            .map(|source| OccurrenceBody { source: source.clone(), world: names.name(source, component, path) })
            .collect();
        if self.placed.len() < PLACEMENT_PATHS_CACHED {
            self.placed.insert(path.to_vec(), bodies.clone());
        }
        bodies
    }
}

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
    /// Hidden names and the bodies the selection names follow the same body and exact
    /// occurrence path through a rename (#9737: a selected body stays selected under its new
    /// name, so Delete hits the body the user actually selected, never one that later takes
    /// the old name). Undo history restores visibility separately. Never parse display labels:
    /// ambiguous labels and names containing slashes use the document's canonical naming.
    pub(crate) fn remap_hidden_bodies(
        &mut self,
        previous: &solidcraft_doc::Document,
        state: &solidcraft_doc::ModelState,
        rename: Option<(&str, &str)>,
    ) {
        let selected: std::collections::BTreeSet<&str> = self.selection.iter().filter_map(|s| s.body_name()).collect();
        if self.visibility.hidden_bodies.is_empty() && selected.is_empty() {
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
        let mut destinations = std::collections::BTreeSet::new();
        let (before, after) = (solidcraft_doc::WorldNames::new(previous, state), solidcraft_doc::WorldNames::new(&self.doc, &renamed));
        for (component, path, _) in previous.placements() {
            for body in state.bodies.iter().filter(|body| previous.model_body_component(body) == component) {
                let old = before.name(&body.name, component, &path);
                let source = rename.filter(|(old, _)| *old == body.name).map_or(body.name.as_str(), |(_, new)| new);
                let new = after.name(source, component, &path);
                if old != new {
                    destinations.insert(new.clone());
                }
                if hidden.contains(old.as_str()) || selected.contains(old.as_str()) {
                    names.insert(old, new);
                }
            }
        }
        self.visibility.hidden_bodies.retain_mut(|hidden| {
            if let Some(new) = names.get(hidden) {
                hidden.clone_from(new);
                true
            } else {
                // A deleted body's hidden name can be reused (#10163). Only a current
                // body's hide follows into that name; a stale entry must not hide it.
                !destinations.contains(hidden)
            }
        });
        for sel in &mut self.selection {
            let name = match sel {
                Sel::Body { name } => name,
                Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => body,
                _ => continue,
            };
            if let Some(new) = names.get(name.as_str()) {
                name.clone_from(new);
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
    /// Resolve only the supplied path, never an exponentially expanded assembly. The placement
    /// index answers from lookups; the plain walk stays as the fallback and the error reporter.
    pub fn occurrence_placement(&self, path: &[u64]) -> Result<OccurrencePlacement> {
        if path.is_empty() || path.len() > 64 {
            return Err(invalid("occurrence path must contain 1..64 ids"));
        }
        if let Some(placement) = self.occurrence_placement_indexed(path) {
            return Ok(placement);
        }
        self.occurrence_placement_walk(path)
    }

    /// [`Self::occurrence_placement`] through [`PlacementIndex`]: the path walk multiplies
    /// transforms as the plain walk does, but occurrences, components and body names are
    /// lookups. `None` for any path the index can't answer, so the walk reports the exact error.
    fn occurrence_placement_indexed(&self, path: &[u64]) -> Option<OccurrencePlacement> {
        let mut guard = self.placement_index()?;
        let index = guard.as_mut()?;
        let mut component = 0;
        let mut world = IDENTITY;
        let mut parent_world = IDENTITY;
        let mut leaf = None;
        for id in path {
            let i = *index.by_parent_id.get(&(component, *id))?;
            let o = &index.doc.occurrences[i];
            if !index.components.contains(&o.component) {
                return None;
            }
            parent_world = world;
            world = mat_mul(&world, self.pending_moves.get(id).unwrap_or(&o.transform));
            if world.iter().flatten().any(|x| !x.is_finite()) {
                return None;
            }
            leaf = Some(i);
            component = o.component;
        }
        let leaf = leaf?;
        let bodies = index.bodies_at(path, component).to_vec();
        let leaf = &index.doc.occurrences[leaf];
        Some(OccurrencePlacement {
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

    /// The placement index for the current document and model, built on first use.
    fn placement_index(&self) -> Option<std::sync::MutexGuard<'_, Option<PlacementIndex>>> {
        let model = self.model.state();
        let mut guard = self.placement_index_cache.lock().ok()?;
        let index = guard.get_or_insert_with(|| PlacementIndex::new(&self.doc, &model));
        if !index.matches(self, &model) {
            *index = PlacementIndex::new(&self.doc, &model);
        }
        Some(guard)
    }

    /// The plain walk over `doc.occurrences`, kept as the fallback for paths the index can't
    /// answer and as the reference the indexed answers are checked against (#7502).
    pub(crate) fn occurrence_placement_walk(&self, path: &[u64]) -> Result<OccurrencePlacement> {
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

    /// Is the leaf of `path` placed inside a component that has more than one placement? Its
    /// transform lives in that shared definition, so moving it moves it in every placement.
    /// Top-level placements and unknown ids are never shared.
    pub fn occurrence_shared(&self, path: &[u64]) -> bool {
        let Some((_, ancestors)) = path.split_last() else { return false };
        ancestors.iter().take(64).any(|id| {
            self.doc
                .occurrences
                .iter()
                .find(|o| o.id == *id)
                .is_some_and(|a| self.doc.occurrences.iter().filter(|o| o.component == a.component).nth(1).is_some())
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
            if cache.as_ref().is_none_or(|c| !c.matches(self, &model)) {
                *cache = Some(OccurrenceBodiesCache {
                    doc: self.doc.clone(),
                    model,
                    pending_moves: self.pending_moves.clone(),
                    bodies: std::collections::HashMap::new(),
                });
            }
            if let Some(cache) = cache.as_mut() {
                if !cache.bodies.contains_key(&key) && cache.bodies.len() >= OCCURRENCE_BODIES_CACHED {
                    // Retire the deepest path first, preferring direct-body answers over
                    // subtree answers at equal depth, then the greatest full key for a
                    // stable tie break. Broader assembly listings survive bursts of per-copy
                    // queries (#7502). Selecting a victim scans at most 256 cached keys,
                    // avoiding extra eviction bookkeeping while keeping the cache bounded.
                    if let Some(evict) =
                        cache.bodies.keys().max_by(|a, b| a.0.len().cmp(&b.0.len()).then_with(|| b.1.cmp(&a.1)).then_with(|| a.cmp(b))).cloned()
                    {
                        cache.bodies.remove(&evict);
                    }
                }
                cache.bodies.insert(key, bodies.clone());
            }
        }
        Ok(bodies)
    }

    fn expand_occurrence_bodies(&self, path: &[u64], descendants: bool) -> Result<Vec<String>> {
        if let Some(out) = self.expand_occurrence_bodies_indexed(path, descendants) {
            return Ok(out);
        }
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

    /// [`Self::expand_occurrence_bodies`] through [`PlacementIndex`]: children and bodies are
    /// lookups and each path is named once, so a listing costs the subtree's size, not the
    /// subtree times the design (#7502). `None` when the index can't answer (a lock miss, an
    /// invalid path, a cycle or a design over the limits), so the walk reports the exact error.
    fn expand_occurrence_bodies_indexed(&self, path: &[u64], descendants: bool) -> Option<Vec<String>> {
        if path.is_empty() {
            return None;
        }
        let mut guard = self.placement_index()?;
        let index = guard.as_mut()?;
        let mut component = 0;
        let mut world = IDENTITY;
        for id in path {
            let i = *index.by_parent_id.get(&(component, *id))?;
            let o = &index.doc.occurrences[i];
            if !index.components.contains(&o.component) {
                return None;
            }
            world = mat_mul(&world, self.pending_moves.get(id).unwrap_or(&o.transform));
            if world.iter().flatten().any(|x| !x.is_finite()) {
                return None;
            }
            component = o.component;
        }
        let mut stack = vec![(path.to_vec(), component, world)];
        let mut out = Vec::new();
        let mut count = 0;
        while let Some((path, component, world)) = stack.pop() {
            count += 1;
            let bodies = index.bodies_at(&path, component);
            if count > 100_000 || out.len() + bodies.len() > 100_000 {
                return None;
            }
            out.extend(bodies.iter().map(|b| b.world.clone()));
            if descendants && path.len() < 64 {
                for child in index.children.get(&component).into_iter().flatten() {
                    if path.contains(child) {
                        return None;
                    }
                    let i = *index.by_parent_id.get(&(component, *child))?;
                    let o = &index.doc.occurrences[i];
                    if !index.components.contains(&o.component) {
                        return None;
                    }
                    let child_world = mat_mul(&world, self.pending_moves.get(child).unwrap_or(&o.transform));
                    if child_world.iter().flatten().any(|x| !x.is_finite()) {
                        return None;
                    }
                    let mut next = path.clone();
                    next.push(*child);
                    stack.push((next, o.component, child_world));
                }
            }
        }
        Some(out)
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

    /// The hidden joints, folders and bodies of the view, as sets: the browser asks about every
    /// joint of the design on every frame, and an answer that walked the hidden lists made a
    /// frame cost grow with the size of the design (#3892). The sets are built when the view
    /// changes and are kept until it changes again.
    fn visibility_index(&self) -> Option<std::sync::MutexGuard<'_, Option<VisibilityIndex>>> {
        let mut cache = self.visibility_index_cache.lock().ok()?;
        let v = &self.visibility;
        if cache.as_ref().is_none_or(|c| !c.matches(self.revision, v)) {
            // The design or the view changed. The sets are rebuilt only when the hidden lists
            // themselves changed: a change of the design alone leaves them alone.
            if cache.as_ref().is_some_and(|c| c.same_lists(v)) {
                if let Some(c) = cache.as_mut() {
                    c.restamp(self.revision, v);
                }
            } else {
                *cache = Some(VisibilityIndex::new(self.revision, v));
            }
        }
        Some(cache)
    }

    /// Whether a joint's glyph is drawn (`browser.visibility`). Hiding a joint only hides its
    /// drawing: it still constrains and drives the parts.
    /// Is the joint shown itself (its own eye; the Joints folder may still hide it)?
    pub fn joint_visible(&self, id: u64) -> bool {
        if let Some(cache) = self.visibility_index() {
            return cache.as_ref().is_none_or(|c| !c.joint_ids.contains(&id));
        }
        // The lock is poisoned: answer from the list itself.
        visibility_work::add(self.visibility.hidden_joints.len() as u64);
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
        if let Some(cache) = self.visibility_index() {
            return cache.as_ref().is_none_or(|c| c.folder_keys.contains(key));
        }
        // The lock is poisoned: answer from the list itself.
        visibility_work::add(self.visibility.hidden_folders.len() as u64);
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
                    && g.has_item(item)
                    // A hidden group hides the groups inside it.
                    && std::iter::successors(Some(g.id), |id| self.doc.browser_group(*id).and_then(|g| g.parent))
                        .take(solidcraft_doc::MAX_GROUP_NESTING + 1)
                        .any(|id| self.folder_hidden(&format!("{scope}/g{id}")))
            })
    }

    /// The placement a sketch or construction feature is drawn at: the active one while its
    /// component is being edited, else the component's first (empty for the design's own).
    fn feature_path(&self, component: u64) -> Vec<u64> {
        match &self.active_occurrence_path {
            _ if component == 0 => Vec::new(),
            Some(p) if self.active_component == component => p.clone(),
            _ => self.doc.first_path(component),
        }
    }

    /// Is the placement a sketch or construction feature is drawn at shown?
    pub fn feature_placement_visible(&self, id: u64) -> bool {
        if self.visibility.hidden_occurrences.is_empty() {
            return true;
        }
        let component = self.doc.feature(id).map_or(0, |f| f.component);
        component == 0 || self.occurrence_visible(&self.feature_path(component))
    }

    /// Is a sketch or construction feature hidden by its folder or group, at the placement it
    /// is drawn at?
    pub fn feature_folder_hidden(&self, id: u64, folder: &str) -> bool {
        if self.visibility.hidden_folders.is_empty() {
            return false;
        }
        let component = self.doc.feature(id).map_or(0, |f| f.component);
        self.item_folder_hidden(&Self::folder_scope(&self.feature_path(component)), component, folder, &id.to_string())
    }

    /// Is construction geometry or an origin item (by name) shown: not hidden itself, nor by
    /// its folder or a group it is in, nor by a hidden placement of its component?
    pub fn construction_visible(&self, name: &str) -> bool {
        if self.visibility.hidden_origin.iter().any(|h| h == name) {
            return false;
        }
        match self.doc.features.iter().find(|f| f.name == name) {
            Some(f) => self.feature_placement_visible(f.id) && !self.feature_folder_hidden(f.id, "construction"),
            None => true,
        }
    }

    pub fn body_visible(&self, name: &str) -> bool {
        if self.body_hidden_itself(name) || self.body_removed_later(name) {
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

    /// Is the body hidden itself (its own eye; a placement, folder or group may still hide it)?
    fn body_hidden_itself(&self, name: &str) -> bool {
        if let Some(cache) = self.visibility_index() {
            return cache.as_ref().is_some_and(|c| c.body_names.contains(name));
        }
        // The lock is poisoned: answer from the list itself.
        visibility_work::add(self.visibility.hidden_bodies.len() as u64);
        self.visibility.hidden_bodies.iter().any(|n| n == name)
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
        // A selected body that is gone cannot be deleted, measured or shown: drop it, so a
        // later body taking its name is never deleted in its place (#9737).
        if self.selection.iter().any(|s| s.body_name().is_some()) {
            let world = self.world_state();
            self.selection.retain(|s| match s.body_name() {
                Some(name) => world.body(name).is_some(),
                None => true,
            });
        }
        let hidden = std::mem::take(&mut self.visibility.hidden_occurrences);
        self.visibility.hidden_occurrences = hidden.into_iter().filter(|p| self.occurrence_placement(p).is_ok()).collect();
    }
}
