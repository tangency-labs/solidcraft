//! Dependency graph engine and topological ripple reorder mapping for the 2D Timeline.
//!
//! Analyzes prerequisite and downstream feature dependencies across the SolidCraft document,
//! computing the valid drag-and-drop corridor `[min_slot, max_slot]` within which any feature
//! or contiguous selection set can be reordered without violating topological constraints.

use solidcraft_engine::doc::construct::{AxisDef, GeoRef, PointDef};
use solidcraft_engine::doc::{Document, Feature, FeatureKind, PlaneRef};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

/// Valid topological insertion bounds for moving a feature or selection set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovableCorridor {
    /// Earliest 0-based timeline slot where the feature/selection can be placed.
    pub min_slot: usize,
    /// Latest 0-based timeline slot where the feature/selection can be placed.
    pub max_slot: usize,
    /// Current 0-based timeline slot of the feature (or first feature of selection).
    pub current_slot: usize,
}

impl MovableCorridor {
    /// Returns true if `target_slot` is within `[min_slot, max_slot]`.
    pub fn is_valid_target(&self, target_slot: usize) -> bool {
        target_slot >= self.min_slot && target_slot <= self.max_slot
    }

    /// Segments the corridor at the rollback marker boundary into active (< marker)
    /// and inactive (>= marker) intervals.
    pub fn split_at_marker(&self, marker: usize) -> CorridorSplit {
        if self.max_slot < marker {
            CorridorSplit::AllActive { min: self.min_slot, max: self.max_slot }
        } else if self.min_slot >= marker {
            CorridorSplit::AllInactive { min: self.min_slot, max: self.max_slot }
        } else {
            CorridorSplit::Spanning {
                active_min: self.min_slot,
                active_max: marker.saturating_sub(1),
                inactive_min: marker,
                inactive_max: self.max_slot,
            }
        }
    }
}

/// Visual segmentation of a movable corridor relative to the rollback marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorridorSplit {
    /// Entire corridor sits to the left of the marker (rendered in green).
    AllActive { min: usize, max: usize },
    /// Entire corridor sits to the right of the marker (rendered in greyscale).
    AllInactive { min: usize, max: usize },
    /// Corridor spans across the marker: active part green, inactive part greyscale.
    Spanning { active_min: usize, active_max: usize, inactive_min: usize, inactive_max: usize },
}

/// A directed acyclic graph (DAG) representing topological dependencies between features.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DependencyGraph {
    /// All feature IDs in timeline sequence order.
    pub features: Vec<u64>,
    /// Feature ID to 0-based timeline index.
    pub id_to_index: HashMap<u64, usize>,
    /// Feature ID -> direct prerequisite feature IDs.
    pub direct_prereqs: HashMap<u64, BTreeSet<u64>>,
    /// Feature ID -> direct downstream dependent feature IDs.
    pub direct_dependents: HashMap<u64, BTreeSet<u64>>,
    /// Feature ID -> transitive closure of all prerequisite feature IDs (ancestors).
    pub all_prereqs: HashMap<u64, BTreeSet<u64>>,
    /// Feature ID -> transitive closure of all downstream dependent feature IDs (descendants).
    pub all_dependents: HashMap<u64, BTreeSet<u64>>,
}

impl DependencyGraph {
    /// Builds the dependency graph from a SolidCraft document.
    pub fn from_doc(doc: &Document) -> Self {
        let n = doc.features.len();
        let mut features = Vec::with_capacity(n);
        let mut id_to_index = HashMap::with_capacity(n);
        let mut name_to_id = HashMap::with_capacity(n);

        for (idx, f) in doc.features.iter().enumerate() {
            features.push(f.id);
            id_to_index.insert(f.id, idx);
            name_to_id.insert(f.name.as_str(), f.id);
        }

        let mut direct_prereqs: HashMap<u64, BTreeSet<u64>> = HashMap::with_capacity(n);
        let mut direct_dependents: HashMap<u64, BTreeSet<u64>> = HashMap::with_capacity(n);
        let mut body_producers: HashMap<String, u64> = HashMap::new();
        let mut creator_to_body: HashMap<u64, String> = HashMap::new();
        let mut active_shell_by_body: HashMap<String, u64> = HashMap::new();
        let mut active_shell_by_component: HashMap<u64, u64> = HashMap::new();

        for f in &doc.features {
            let mut prereqs = BTreeSet::new();

            // 1. Direct input sketches (profiles, paths, guides, holes, etc.)
            for sketch_id in f.kind.input_sketches() {
                if sketch_id != f.id && id_to_index.contains_key(&sketch_id) {
                    prereqs.insert(sketch_id);
                }
            }

            // 2. Persistent face and edge references (e.g. F<id>:face)
            for name in &f.face_names {
                if let Some(creator) = solidcraft_engine::doc::persistent_name::made_by(name)
                    && creator != f.id
                    && id_to_index.contains_key(&creator)
                {
                    prereqs.insert(creator);
                }
            }
            for name in &f.edge_names {
                if let Some(creator) = solidcraft_engine::doc::persistent_name::made_by(name)
                    && creator != f.id
                    && id_to_index.contains_key(&creator)
                {
                    prereqs.insert(creator);
                }
            }

            // 3. Variant-specific feature dependencies
            match &f.kind {
                FeatureKind::Pattern { features: pat_feats, bodies, .. } => {
                    for feat in pat_feats {
                        if feat.0 != f.id && id_to_index.contains_key(&feat.0) {
                            prereqs.insert(feat.0);
                        }
                    }
                    for b in bodies {
                        if let Some(&creator) = body_producers.get(b.as_ref())
                            && creator != f.id
                        {
                            prereqs.insert(creator);
                        }
                    }
                }
                FeatureKind::Mirror { features: mir_feats, plane, bodies, .. } => {
                    for feat in mir_feats {
                        if feat.0 != f.id && id_to_index.contains_key(&feat.0) {
                            prereqs.insert(feat.0);
                        }
                    }
                    for b in bodies {
                        if let Some(&creator) = body_producers.get(b.as_ref())
                            && creator != f.id
                        {
                            prereqs.insert(creator);
                        }
                    }
                    collect_plane_prereqs(plane, &mut prereqs, &name_to_id, &body_producers);
                }
                FeatureKind::ConstructionPlane { plane } => {
                    collect_plane_prereqs(plane, &mut prereqs, &name_to_id, &body_producers);
                }
                FeatureKind::ConstructionAxis { def } => {
                    collect_axis_prereqs(def, &mut prereqs, &name_to_id, &body_producers);
                }
                FeatureKind::ConstructionPoint { def } => {
                    collect_point_prereqs(def, &mut prereqs, &name_to_id, &body_producers);
                }
                FeatureKind::Sketch { plane, sketch } => {
                    collect_plane_prereqs(plane, &mut prereqs, &name_to_id, &body_producers);
                    for link in &sketch.links {
                        if let Some(face) = &link.face_name
                            && let Some(creator) = solidcraft_engine::doc::persistent_name::made_by(face)
                            && creator != f.id
                            && id_to_index.contains_key(&creator)
                        {
                            prereqs.insert(creator);
                        }
                        if let Some(edge) = &link.edge_name
                            && let Some(creator) = solidcraft_engine::doc::persistent_name::made_by(edge)
                            && creator != f.id
                            && id_to_index.contains_key(&creator)
                        {
                            prereqs.insert(creator);
                        }
                        collect_link_source_prereqs(&link.source, &mut prereqs, &name_to_id, &body_producers);
                    }
                }
                FeatureKind::Split { body, plane, sketch, .. } => {
                    if let Some(&creator) = body_producers.get(body.as_ref())
                        && creator != f.id
                    {
                        prereqs.insert(creator);
                    }
                    collect_plane_prereqs(plane, &mut prereqs, &name_to_id, &body_producers);
                    if let Some(sk) = sketch
                        && sk.0 != f.id
                        && id_to_index.contains_key(&sk.0)
                    {
                        prereqs.insert(sk.0);
                    }
                }
                FeatureKind::SplitFace { plane, sketch, .. } => {
                    collect_plane_prereqs(plane, &mut prereqs, &name_to_id, &body_producers);
                    if let Some(sk) = sketch
                        && sk.0 != f.id
                        && id_to_index.contains_key(&sk.0)
                    {
                        prereqs.insert(sk.0);
                    }
                }
                FeatureKind::Combine { target, tools, .. } => {
                    if let Some(&creator) = body_producers.get(target.as_ref())
                        && creator != f.id
                    {
                        prereqs.insert(creator);
                    }
                    for tool in tools {
                        if let Some(&creator) = body_producers.get(tool.as_ref())
                            && creator != f.id
                        {
                            prereqs.insert(creator);
                        }
                    }
                }
                FeatureKind::Lip { body, .. } => {
                    let target_body = body.as_deref().map(str::to_string).or_else(|| {
                        f.face_names.iter().find_map(|name| {
                            let creator = solidcraft_engine::doc::persistent_name::made_by(name)?;
                            creator_to_body.get(&creator).cloned()
                        })
                    });
                    if let Some(b) = &target_body {
                        if let Some(&creator) = body_producers.get(b)
                            && creator != f.id
                            && id_to_index.contains_key(&creator)
                        {
                            prereqs.insert(creator);
                        }
                        if let Some(&shell_id) = active_shell_by_body.get(b)
                            && shell_id != f.id
                            && id_to_index.contains_key(&shell_id)
                        {
                            prereqs.insert(shell_id);
                        } else if let Some(&shell_id) = active_shell_by_component.get(&f.component)
                            && shell_id != f.id
                            && id_to_index.contains_key(&shell_id)
                        {
                            prereqs.insert(shell_id);
                        }
                    } else if let Some(&shell_id) = active_shell_by_component.get(&f.component)
                        && shell_id != f.id
                        && id_to_index.contains_key(&shell_id)
                    {
                        prereqs.insert(shell_id);
                    }
                }
                FeatureKind::Shell { body, .. } => {
                    let target_body = body.as_deref().map(str::to_string).or_else(|| {
                        f.face_names.iter().find_map(|name| {
                            let creator = solidcraft_engine::doc::persistent_name::made_by(name)?;
                            creator_to_body.get(&creator).cloned()
                        })
                    });
                    if let Some(b) = &target_body
                        && let Some(&creator) = body_producers.get(b)
                        && creator != f.id
                        && id_to_index.contains_key(&creator)
                    {
                        prereqs.insert(creator);
                    }
                }
                _ => {}
            }

            // 4. Body reference & modification tracking
            for participant in &f.participants {
                if let Some(&creator) = body_producers.get(participant.as_str())
                    && creator != f.id
                {
                    prereqs.insert(creator);
                }
            }

            // Update body producers map
            for b in &f.body_names {
                let trimmed = b.trim();
                if !trimmed.is_empty() {
                    body_producers.insert(trimmed.to_string(), f.id);
                    creator_to_body.insert(f.id, trimmed.to_string());
                }
            }
            if f.body_names.is_empty() && creates_body(f) {
                body_producers.insert(f.name.clone(), f.id);
                creator_to_body.insert(f.id, f.name.clone());
            }
            for participant in &f.participants {
                let trimmed = participant.trim();
                if !trimmed.is_empty() {
                    body_producers.insert(trimmed.to_string(), f.id);
                }
            }

            if let FeatureKind::Shell { body, .. } = &f.kind {
                let target_body = body.as_deref().map(str::to_string).or_else(|| {
                    f.face_names.iter().find_map(|name| {
                        let creator = solidcraft_engine::doc::persistent_name::made_by(name)?;
                        creator_to_body.get(&creator).cloned()
                    })
                });
                if let Some(b) = target_body {
                    active_shell_by_body.insert(b, f.id);
                }
                active_shell_by_component.insert(f.component, f.id);
            }

            // The helpers above do not check what they add: a feature is never its own
            // prerequisite, and only features of this design count.
            prereqs.remove(&f.id);
            prereqs.retain(|p| id_to_index.contains_key(p));

            // Record direct prereqs and inverted dependents
            for &p in &prereqs {
                direct_dependents.entry(p).or_default().insert(f.id);
            }
            direct_prereqs.insert(f.id, prereqs);
        }

        // 5. Compute transitive closures (all_prereqs & all_dependents)
        let mut all_prereqs: HashMap<u64, BTreeSet<u64>> = HashMap::with_capacity(n);
        let mut all_dependents: HashMap<u64, BTreeSet<u64>> = HashMap::with_capacity(n);

        for &id in &features {
            let mut ancestors = BTreeSet::new();
            let mut queue = VecDeque::new();
            let mut visited = HashSet::new();

            if let Some(direct) = direct_prereqs.get(&id) {
                for &p in direct {
                    queue.push_back(p);
                    visited.insert(p);
                }
            }

            while let Some(curr) = queue.pop_front() {
                ancestors.insert(curr);
                if let Some(next_direct) = direct_prereqs.get(&curr) {
                    for &next_p in next_direct {
                        if visited.insert(next_p) {
                            queue.push_back(next_p);
                        }
                    }
                }
            }

            for &a in &ancestors {
                all_dependents.entry(a).or_default().insert(id);
            }
            all_prereqs.insert(id, ancestors);
        }

        Self { features, id_to_index, direct_prereqs, direct_dependents, all_prereqs, all_dependents }
    }

    /// Computes the valid movable corridor `[min_slot, max_slot]` for a single feature.
    pub fn corridor_for(&self, id: u64) -> Option<MovableCorridor> {
        let &curr_slot = self.id_to_index.get(&id)?;
        let total = self.features.len();

        let p_max = self.all_prereqs.get(&id).into_iter().flatten().filter_map(|&p| self.id_to_index.get(&p).copied()).max();

        let d_min = self.all_dependents.get(&id).into_iter().flatten().filter_map(|&d| self.id_to_index.get(&d).copied()).min();

        let min_slot = p_max.map(|p| p.saturating_add(1)).unwrap_or(0);
        let max_slot = d_min.map(|d| d.saturating_sub(1)).unwrap_or(total.saturating_sub(1));

        Some(MovableCorridor { min_slot: min_slot.min(curr_slot), max_slot: max_slot.max(curr_slot), current_slot: curr_slot })
    }

    /// Computes the valid movable corridor `[min_slot, max_slot]` for a contiguous selection set.
    pub fn corridor_for_selection(&self, ids: &[u64]) -> Option<MovableCorridor> {
        if ids.is_empty() {
            return None;
        }

        let total = self.features.len();
        let selected_set: HashSet<u64> = ids.iter().copied().collect();
        let mut min_curr = total;
        let mut max_curr = 0;

        for &id in ids {
            let &slot = self.id_to_index.get(&id)?;
            min_curr = min_curr.min(slot);
            max_curr = max_curr.max(slot);
        }

        let p_max = ids
            .iter()
            .filter_map(|id| self.all_prereqs.get(id))
            .flatten()
            .filter(|p| !selected_set.contains(p))
            .filter_map(|p| self.id_to_index.get(p).copied())
            .max();

        let d_min = ids
            .iter()
            .filter_map(|id| self.all_dependents.get(id))
            .flatten()
            .filter(|d| !selected_set.contains(d))
            .filter_map(|d| self.id_to_index.get(d).copied())
            .min();

        let min_slot = p_max.map(|p| p.saturating_add(1)).unwrap_or(0);
        let max_slot = d_min.map(|d| d.saturating_sub(1)).unwrap_or(total.saturating_sub(1));

        Some(MovableCorridor { min_slot: min_slot.min(min_curr), max_slot: max_slot.max(max_curr), current_slot: min_curr })
    }

    /// Returns true if `other_id` is a prerequisite (ancestor) of `selected_id`.
    pub fn is_prerequisite(&self, selected_id: u64, other_id: u64) -> bool {
        self.all_prereqs.get(&selected_id).is_some_and(|set| set.contains(&other_id))
    }

    /// Returns true if `other_id` is a downstream dependent (descendant) of `selected_id`.
    pub fn is_dependent(&self, selected_id: u64, other_id: u64) -> bool {
        self.all_dependents.get(&selected_id).is_some_and(|set| set.contains(&other_id))
    }

    /// Returns all prerequisite (ancestor) feature IDs of `id`.
    pub fn prerequisites_of(&self, id: u64) -> Option<&BTreeSet<u64>> {
        self.all_prereqs.get(&id)
    }

    /// Returns all downstream dependent (descendant) feature IDs of `id`.
    pub fn dependents_of(&self, id: u64) -> Option<&BTreeSet<u64>> {
        self.all_dependents.get(&id)
    }

    /// Returns true if this feature is directly suppressed OR transitively suppressed by any of its prerequisite ancestors.
    pub fn is_suppressed(&self, doc: &Document, id: u64) -> bool {
        if doc.feature(id).is_some_and(|f| f.suppressed) {
            return true;
        }
        if let Some(prereqs) = self.all_prereqs.get(&id) {
            for &p in prereqs {
                if doc.feature(p).is_some_and(|f| f.suppressed) {
                    return true;
                }
            }
        }
        false
    }

    /// Collects `id` and all of its downstream dependent features that must be suppressed along with it.
    pub fn collect_suppression_cascade(&self, id: u64) -> Vec<u64> {
        self.collect_suppression_cascade_for_set(&[id])
    }

    /// Collects all features in `ids` and all of their downstream dependent features.
    pub fn collect_suppression_cascade_for_set(&self, ids: &[u64]) -> Vec<u64> {
        let mut set = BTreeSet::new();
        for &id in ids {
            set.insert(id);
            if let Some(deps) = self.all_dependents.get(&id) {
                for &d in deps {
                    set.insert(d);
                }
            }
        }
        set.into_iter().collect()
    }

    /// Collects `id`, all of its currently-suppressed prerequisite ancestors, and all of its downstream dependents.
    pub fn collect_unsuppression_cascade(&self, doc: &Document, id: u64) -> Vec<u64> {
        self.collect_unsuppression_cascade_for_set(doc, &[id])
    }

    /// Collects all features in `ids`, all of their currently-suppressed prerequisite ancestors,
    /// and all of their downstream dependents.
    pub fn collect_unsuppression_cascade_for_set(&self, doc: &Document, ids: &[u64]) -> Vec<u64> {
        let mut set = BTreeSet::new();
        for &id in ids {
            set.insert(id);
            if let Some(prereqs) = self.all_prereqs.get(&id) {
                for &p in prereqs {
                    if doc.feature(p).is_some_and(|f| f.suppressed) {
                        set.insert(p);
                    }
                }
            }
            if let Some(dependents) = self.all_dependents.get(&id) {
                for &d in dependents {
                    set.insert(d);
                }
            }
        }
        set.into_iter().collect()
    }

    /// Collects all features in `ids`, their currently-suppressed prerequisite ancestors,
    /// and downstream dependents according to the unsuppress mode and separately-suppressed memory.
    pub fn collect_unsuppression_with_mode(
        &self,
        doc: &Document,
        ids: &[u64],
        mode: crate::prefs::Timeline2dUnsuppressMode,
        separately_suppressed: &BTreeSet<u64>,
    ) -> Vec<u64> {
        match mode {
            crate::prefs::Timeline2dUnsuppressMode::UnsuppressAll => self.collect_unsuppression_cascade_for_set(doc, ids),
            crate::prefs::Timeline2dUnsuppressMode::RestoreState => {
                let mut unsuppressed_set = BTreeSet::new();
                for &id in ids {
                    unsuppressed_set.insert(id);
                    if let Some(prereqs) = self.all_prereqs.get(&id) {
                        for &p in prereqs {
                            if doc.feature(p).is_some_and(|f| f.suppressed) {
                                unsuppressed_set.insert(p);
                            }
                        }
                    }
                }

                // Gather all candidate downstream features from the newly unsuppressed features
                let mut candidate_deps = BTreeSet::new();
                for &u in &unsuppressed_set {
                    if let Some(deps) = self.all_dependents.get(&u) {
                        for &d in deps {
                            if doc.feature(d).is_some_and(|f| f.suppressed) && !unsuppressed_set.contains(&d) {
                                candidate_deps.insert(d);
                            }
                        }
                    }
                }

                // Sort candidate dependents by timeline step order to evaluate DAG in topological order
                let mut sorted_candidates: Vec<u64> = candidate_deps.into_iter().collect();
                sorted_candidates.sort_by_key(|&fid| self.id_to_index.get(&fid).copied().unwrap_or(usize::MAX));

                for dep in sorted_candidates {
                    // If separately suppressed, user intended for it to stay suppressed
                    if separately_suppressed.contains(&dep) {
                        continue;
                    }
                    // Check if all prerequisites of dep are (or will be) unsuppressed
                    let all_prereqs_ok = if let Some(prereqs) = self.all_prereqs.get(&dep) {
                        prereqs.iter().all(|&p| !doc.feature(p).is_some_and(|f| f.suppressed) || unsuppressed_set.contains(&p))
                    } else {
                        true
                    };

                    if all_prereqs_ok {
                        unsuppressed_set.insert(dep);
                    }
                }

                unsuppressed_set.into_iter().collect()
            }
        }
    }

    /// Computes the ripple push reorder mapping for array splicing:
    /// Moving `count` items from `from_slot` to `to_slot`.
    /// Returns a list of `(source_index, target_index)` shifts for all affected indices.
    pub fn ripple_shifts(from_slot: usize, to_slot: usize, total: usize) -> Vec<(usize, usize)> {
        let mut shifts = Vec::new();
        if from_slot == to_slot || from_slot >= total || to_slot >= total {
            return shifts;
        }

        if from_slot < to_slot {
            // Moving right: items in (from_slot, to_slot] shift left by 1
            for i in (from_slot + 1)..=to_slot {
                shifts.push((i, i - 1));
            }
            shifts.push((from_slot, to_slot));
        } else {
            // Moving left: items in [to_slot, from_slot) shift right by 1
            for i in to_slot..from_slot {
                shifts.push((i, i + 1));
            }
            shifts.push((from_slot, to_slot));
        }
        shifts
    }
}

/// Helper: whether a feature kind typically creates a new solid body when `body_names` is empty.
fn creates_body(f: &Feature) -> bool {
    matches!(
        f.kind,
        FeatureKind::Extrude { .. }
            | FeatureKind::Revolve { .. }
            | FeatureKind::Sweep { .. }
            | FeatureKind::Loft { .. }
            | FeatureKind::Box { .. }
            | FeatureKind::Cylinder { .. }
            | FeatureKind::Sphere { .. }
            | FeatureKind::Torus { .. }
            | FeatureKind::Cone { .. }
            | FeatureKind::Pipe { .. }
            | FeatureKind::Coil { .. }
            | FeatureKind::Import { .. }
    )
}

/// Recursively collects prerequisites from a `PlaneRef`.
fn collect_plane_prereqs(plane: &PlaneRef, prereqs: &mut BTreeSet<u64>, name_to_id: &HashMap<&str, u64>, body_producers: &HashMap<String, u64>) {
    match plane {
        PlaneRef::Construction { feature } => {
            prereqs.insert(feature.0);
        }
        PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => {
            collect_plane_prereqs(base, prereqs, name_to_id, body_producers);
        }
        PlaneRef::ThroughPoint { base, point } => {
            collect_plane_prereqs(base, prereqs, name_to_id, body_producers);
            collect_geo_prereqs(point, prereqs, name_to_id, body_producers);
        }
        PlaneRef::AtAngle { base, line, .. } => {
            collect_plane_prereqs(base, prereqs, name_to_id, body_producers);
            if let Some(geo) = line {
                collect_geo_prereqs(geo, prereqs, name_to_id, body_producers);
            }
        }
        PlaneRef::Midplane { a, b } | PlaneRef::TwoEdges { a, b } | PlaneRef::Perpendicular { plane: a, line: b } => {
            collect_geo_prereqs(a, prereqs, name_to_id, body_producers);
            collect_geo_prereqs(b, prereqs, name_to_id, body_producers);
        }
        PlaneRef::ThreePoints { a, b, c } => {
            collect_geo_prereqs(a, prereqs, name_to_id, body_producers);
            collect_geo_prereqs(b, prereqs, name_to_id, body_producers);
            collect_geo_prereqs(c, prereqs, name_to_id, body_producers);
        }
        PlaneRef::Tangent { face, .. } | PlaneRef::AlongPath { path: face, .. } => {
            collect_geo_prereqs(face, prereqs, name_to_id, body_producers);
        }
        _ => {}
    }
}

/// Collects prerequisites from a `GeoRef`.
fn collect_geo_prereqs(geo: &GeoRef, prereqs: &mut BTreeSet<u64>, name_to_id: &HashMap<&str, u64>, body_producers: &HashMap<String, u64>) {
    match geo {
        GeoRef::SketchPoint { sketch, .. } | GeoRef::SketchCurve { sketch, .. } => {
            prereqs.insert(*sketch);
        }
        GeoRef::Axis { name } | GeoRef::ConstructPoint { name } => {
            if let Some(&id) = name_to_id.get(name.as_ref()) {
                prereqs.insert(id);
            }
        }
        GeoRef::Face { name: Some(name), .. } | GeoRef::Edge { name: Some(name), .. } => {
            if let Some(creator) = solidcraft_engine::doc::persistent_name::made_by(name) {
                prereqs.insert(creator);
            }
        }
        GeoRef::Vertex { edges: Some(edges), .. } => {
            for edge in edges {
                if let Some(creator) = solidcraft_engine::doc::persistent_name::made_by(edge) {
                    prereqs.insert(creator);
                }
            }
        }
        GeoRef::Plane { plane } => {
            collect_plane_prereqs(plane, prereqs, name_to_id, body_producers);
        }
        _ => {}
    }
}

/// Collects prerequisites from an `AxisDef`.
fn collect_axis_prereqs(def: &AxisDef, prereqs: &mut BTreeSet<u64>, name_to_id: &HashMap<&str, u64>, body_producers: &HashMap<String, u64>) {
    match def {
        AxisDef::ThroughFace { face } | AxisDef::Edge { edge: face } => {
            collect_geo_prereqs(face, prereqs, name_to_id, body_producers);
        }
        AxisDef::NormalToFace { face, point } => {
            collect_geo_prereqs(face, prereqs, name_to_id, body_producers);
            collect_geo_prereqs(point, prereqs, name_to_id, body_producers);
        }
        AxisDef::TwoPlanes { a, b } | AxisDef::TwoPoints { a, b } => {
            collect_geo_prereqs(a, prereqs, name_to_id, body_producers);
            collect_geo_prereqs(b, prereqs, name_to_id, body_producers);
        }
    }
}

/// Collects prerequisites from a `PointDef`.
fn collect_point_prereqs(def: &PointDef, prereqs: &mut BTreeSet<u64>, name_to_id: &HashMap<&str, u64>, body_producers: &HashMap<String, u64>) {
    match def {
        PointDef::At { point } | PointDef::Center { of: point } | PointDef::AlongPath { path: point, .. } => {
            collect_geo_prereqs(point, prereqs, name_to_id, body_producers);
        }
        PointDef::TwoEdges { a, b } | PointDef::EdgeAndPlane { edge: a, plane: b } => {
            collect_geo_prereqs(a, prereqs, name_to_id, body_producers);
            collect_geo_prereqs(b, prereqs, name_to_id, body_producers);
        }
        PointDef::ThreePlanes { a, b, c } => {
            collect_geo_prereqs(a, prereqs, name_to_id, body_producers);
            collect_geo_prereqs(b, prereqs, name_to_id, body_producers);
            collect_geo_prereqs(c, prereqs, name_to_id, body_producers);
        }
    }
}

/// Collects prerequisites from a sketch `LinkSource`.
fn collect_link_source_prereqs(
    src: &solidcraft_engine::sketch::LinkSource,
    prereqs: &mut BTreeSet<u64>,
    name_to_id: &HashMap<&str, u64>,
    body_producers: &HashMap<String, u64>,
) {
    use solidcraft_engine::sketch::LinkSource;
    match src {
        LinkSource::SketchCurve { sketch, .. } | LinkSource::SketchPoint { sketch, .. } | LinkSource::OnSurface { sketch, .. } => {
            prereqs.insert(*sketch);
        }
        LinkSource::Plane { name } => {
            if let Some(&id) = name_to_id.get(name.as_str()) {
                prereqs.insert(id);
            }
        }
        LinkSource::Edge { body, .. }
        | LinkSource::Face { body, .. }
        | LinkSource::Body { body }
        | LinkSource::Vertex { body, .. }
        | LinkSource::Iso { body, .. }
        | LinkSource::Spun { body, .. } => {
            if let Some(&id) = body_producers.get(body.as_str()) {
                prereqs.insert(id);
            }
        }
        LinkSource::Intersection { a, b } => {
            collect_link_source_prereqs(a, prereqs, name_to_id, body_producers);
            collect_link_source_prereqs(b, prereqs, name_to_id, body_producers);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plane that names itself, or a feature that is not in the design, is no prerequisite:
    /// the feature keeps a corridor wider than its own step and is nobody's dependent.
    #[test]
    fn a_feature_is_never_its_own_prerequisite() {
        let mut s = solidcraft_engine::Session::default();
        for (cmd, p) in [
            ("solid.box", serde_json::json!({"length": 10, "width": 10, "height": 10})),
            ("construct.plane.offset", serde_json::json!({"base": "XY", "offset": 5})),
            ("solid.box", serde_json::json!({"length": 5, "width": 5, "height": 5, "corner": [20, 0, 0]})),
        ] {
            s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"));
        }
        let mut doc = s.doc.as_ref().clone();
        let plane_id = doc.features[1].id;
        let FeatureKind::ConstructionPlane { plane: PlaneRef::Offset { base, .. } } = &mut doc.features[1].kind else {
            panic!("not an offset construction plane")
        };
        **base = PlaneRef::Construction { feature: plane_id.into() };
        let graph = DependencyGraph::from_doc(&doc);
        assert!(!graph.is_prerequisite(plane_id, plane_id));
        assert!(!graph.is_dependent(plane_id, plane_id));
        let corridor = graph.corridor_for(plane_id).unwrap();
        assert!(corridor.min_slot < corridor.max_slot, "the plane can still move: {corridor:?}");
    }

    #[test]
    fn test_corridor_is_valid_target() {
        let corridor = MovableCorridor { min_slot: 2, max_slot: 5, current_slot: 3 };
        assert!(!corridor.is_valid_target(1));
        assert!(corridor.is_valid_target(2));
        assert!(corridor.is_valid_target(3));
        assert!(corridor.is_valid_target(5));
        assert!(!corridor.is_valid_target(6));
    }

    #[test]
    fn test_corridor_split_at_marker() {
        let corridor = MovableCorridor { min_slot: 2, max_slot: 7, current_slot: 4 };

        // Marker after corridor: all active
        assert_eq!(corridor.split_at_marker(8), CorridorSplit::AllActive { min: 2, max: 7 });

        // Marker before corridor: all inactive
        assert_eq!(corridor.split_at_marker(2), CorridorSplit::AllInactive { min: 2, max: 7 });

        // Marker inside corridor: spanning
        assert_eq!(corridor.split_at_marker(5), CorridorSplit::Spanning { active_min: 2, active_max: 4, inactive_min: 5, inactive_max: 7 });
    }

    #[test]
    fn test_ripple_shifts_moving_right() {
        // Moving index 1 to index 4 in a 6-item list
        let shifts = DependencyGraph::ripple_shifts(1, 4, 6);
        assert_eq!(shifts, vec![(2, 1), (3, 2), (4, 3), (1, 4)]);
    }

    #[test]
    fn test_ripple_shifts_moving_left() {
        // Moving index 4 to index 1 in a 6-item list
        let shifts = DependencyGraph::ripple_shifts(4, 1, 6);
        assert_eq!(shifts, vec![(1, 2), (2, 3), (3, 4), (4, 1)]);
    }

    /// Verifies dependency graph calculations, prerequisite/dependent sets, and movable corridor slot bounds.
    #[test]
    fn test_dependency_chain_and_corridor() {
        // Build mock features: 0 (sketch) -> 1 (extrude) -> 2 (fillet) -> 3 (independent)
        let mut graph = DependencyGraph { features: vec![100, 101, 102, 103], ..Default::default() };
        graph.id_to_index.insert(100, 0);
        graph.id_to_index.insert(101, 1);
        graph.id_to_index.insert(102, 2);
        graph.id_to_index.insert(103, 3);

        let mut prereqs_101 = BTreeSet::new();
        prereqs_101.insert(100);
        graph.all_prereqs.insert(101, prereqs_101);

        let mut prereqs_102 = BTreeSet::new();
        prereqs_102.insert(100);
        prereqs_102.insert(101);
        graph.all_prereqs.insert(102, prereqs_102);

        let mut dependents_100 = BTreeSet::new();
        dependents_100.insert(101);
        dependents_100.insert(102);
        graph.all_dependents.insert(100, dependents_100);

        let mut dependents_101 = BTreeSet::new();
        dependents_101.insert(102);
        graph.all_dependents.insert(101, dependents_101);

        // Corridor for 101 (Extrude):
        // Prereq is 100 at slot 0 -> p_max = 0 -> min_slot = 1
        // Dependent is 102 at slot 2 -> d_min = 2 -> max_slot = 1
        let corr_101 = graph.corridor_for(101).expect("corridor for 101");
        assert_eq!(corr_101.min_slot, 1);
        assert_eq!(corr_101.max_slot, 1);

        // Corridor for 103 (Independent):
        // No prereqs, no dependents -> can move anywhere from 0 to 3
        let corr_103 = graph.corridor_for(103).expect("corridor for 103");
        assert_eq!(corr_103.min_slot, 0);
        assert_eq!(corr_103.max_slot, 3);

        // Corridor for 102 (Fillet):
        // Prereqs are 100 (0) and 101 (1) -> p_max = 1 -> min_slot = 2
        // No dependents -> max_slot = 3
        let corr_102 = graph.corridor_for(102).expect("corridor for 102");
        assert_eq!(corr_102.min_slot, 2);
        assert_eq!(corr_102.max_slot, 3);

        // Query helpers
        assert!(graph.is_prerequisite(102, 100));
        assert!(graph.is_prerequisite(102, 101));
        assert!(!graph.is_prerequisite(102, 103));
        assert!(graph.is_dependent(100, 102));
        assert!(!graph.is_dependent(103, 102));
    }

    /// Verifies topological movable corridor calculation on the Plastic Enclosure sample part.
    #[test]
    fn test_plastic_enclosure_movable_corridor() {
        let sample = solidcraft_engine::sample::samples().into_iter().find(|s| s.name == "Plastic Enclosure").expect("Plastic Enclosure sample");
        let script = (sample.script)();
        let app = crate::test_support::app_with_script(script);
        let graph = DependencyGraph::from_doc(&app.session.doc);

        let box_id = app.session.doc.features[0].id;
        let shell_id = app.session.doc.features[1].id;
        let lip_id = app.session.doc.features[2].id;

        // Lip1 requires the shelled state produced by Shell1 on the body
        assert!(graph.is_prerequisite(lip_id, shell_id));
        assert!(graph.is_prerequisite(lip_id, box_id));
        assert!(graph.is_dependent(shell_id, lip_id));

        // Shell1 cannot be reordered to or past Lip1's slot (slot 2)
        let shell_corridor = graph.corridor_for(shell_id).expect("corridor for shell");
        assert_eq!(shell_corridor.min_slot, 1);
        assert_eq!(shell_corridor.max_slot, 1);

        // Lip1 cannot be reordered before Shell1 (slot 1)
        let lip_corridor = graph.corridor_for(lip_id).expect("corridor for lip");
        assert_eq!(lip_corridor.min_slot, 2);
        assert_eq!(lip_corridor.max_slot, 6);
    }
}
