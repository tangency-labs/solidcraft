//! The vertical History's row plan: which folder headers, feature rows and joint rows come where.
//!
//! Working this out from the design took every frame several passes over all features for each
//! folder (and a search for each folder's last feature at every row), which a long history with
//! folders paid for on every repaint, even with the pointer only moving. Everything here follows
//! from the design alone, so it is built once per design change ([`RowPlan::of`]) and kept in the
//! egui context; what the open folders and folds change ([`RowPlan::order`]) is kept beside it
//! until one of them opens or closes. The functions it replaced live on in the tests as the
//! reference it is compared with.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use solidcraft_engine::doc::TimelineGroup;

use super::{TimelineItem, group_path, sketches_key};
use crate::SolveApp;
use crate::history_rows::{HistoryFeature, HistoryRows};
use crate::timeline::{TimelineKind, icon_of};

/// A feature as its row draws it: id, name, icon, suppressed, type name and kind.
pub(super) type FeatureInfo = (u64, String, &'static str, bool, &'static str, TimelineKind);

/// What a plan was built from. The design changes through commands, which count the session's
/// revision up; the marker and the counts guard edits made in place without one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlanKey {
    session: u64,
    revision: u64,
    document: usize,
    marker: Option<usize>,
    features: usize,
    groups: usize,
    under_feature: bool,
}

impl PlanKey {
    fn of(app: &SolveApp, under_feature: bool) -> Self {
        let doc = &app.session.doc;
        Self {
            session: app.session.uid,
            revision: app.session.revision,
            document: Arc::as_ptr(doc) as usize,
            marker: doc.marker,
            features: doc.features.len(),
            groups: doc.timeline_groups.len(),
            under_feature,
        }
    }
}

/// The folders and folds that are open: the visible order follows from these and the plan.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Folds {
    /// Folders closed by the person, by id.
    closed: HashSet<u64>,
    /// Features whose listed sketches are shown.
    open_hosts: BTreeSet<u64>,
}

impl Folds {
    pub(super) fn is_closed(&self, group: u64) -> bool {
        self.closed.contains(&group)
    }
}

pub(super) struct RowPlan {
    key: PlanKey,
    pub features: Vec<FeatureInfo>,
    pub groups: Vec<TimelineGroup>,
    /// The rows to draw (`drawn_rows`).
    pub rows: HistoryRows,
    /// Each feature id's first timeline position.
    index_of: HashMap<u64, usize>,
    /// The folder each feature is in (`group_for`), by timeline position.
    pub group_of: Vec<Option<u64>>,
    /// Whether the sketch at each timeline position has patterns or projections to fold.
    pub has_patterns: Vec<bool>,
    /// The first folder with each id: the one `groups.iter().find` would give.
    by_id: HashMap<u64, usize>,
    /// By folder index: its nesting depth.
    pub depth: Vec<usize>,
    /// By folder index: the position of its first own or nested feature.
    pub first: Vec<Option<usize>>,
    /// By folder index: where its header goes, its own first feature or that of the nearest
    /// ancestor that has one.
    pub position: Vec<Option<usize>>,
    /// By folder index: the position after its last own or nested feature (the end with none).
    pub end: Vec<usize>,
    /// By timeline position: the folders whose header goes before it, outermost first.
    pub starting: Vec<Vec<usize>>,
    /// By timeline position: the folders whose last feature it is, innermost first.
    pub ending: Vec<Vec<usize>>,
    /// Every folder in the order of headers left over after the features.
    pub trailing: Vec<usize>,
    /// By folder index: its own and nested features in timeline order (`folder_features`).
    pub contents: Vec<Vec<u64>>,
    /// By folder index: its own and nested folders' joints.
    pub joints: Vec<Vec<u64>>,
    /// Each folder and parent id, with its fold key in the tree state.
    fold_keys: Vec<(u64, String)>,
    /// Features with sketches listed under them, and their fold keys.
    host_keys: Vec<(u64, String)>,
}

impl RowPlan {
    /// The plan for the design as it is, kept until it changes.
    pub(super) fn cached(app: &SolveApp, ui: &egui::Ui, under_feature: bool) -> Arc<Self> {
        let id = egui::Id::new("sc_history_row_plan");
        let key = PlanKey::of(app, under_feature);
        if let Some(plan) = ui.data(|data| data.get_temp::<Arc<Self>>(id)).filter(|plan| plan.key == key) {
            return plan;
        }
        let plan = Arc::new(Self::of(app, under_feature));
        ui.data_mut(|data| data.insert_temp(id, plan.clone()));
        plan
    }

    /// Build the plan from the design.
    pub(super) fn of(app: &SolveApp, under_feature: bool) -> Self {
        let doc = &app.session.doc;
        let features: Vec<FeatureInfo> = doc
            .features
            .iter()
            .map(|feature| {
                (
                    feature.id,
                    feature.name.clone(),
                    icon_of(&feature.kind),
                    feature.suppressed,
                    feature.kind.type_name(),
                    TimelineKind::of(&feature.kind),
                )
            })
            .collect();
        let groups = doc.timeline_groups.clone();
        let rows = if under_feature {
            let listed: Vec<HistoryFeature> = doc.features.iter().map(HistoryFeature::of).collect();
            crate::history_rows::rows(&listed, &groups, doc.marker, true)
        } else {
            let sketch = |feature: &solidcraft_engine::doc::Feature| matches!(feature.kind, solidcraft_engine::doc::FeatureKind::Sketch { .. });
            HistoryRows::in_order(doc.features.iter().map(|feature| (feature.id, sketch(feature))))
        };
        let mut index_of = HashMap::with_capacity(features.len());
        for (index, feature) in features.iter().enumerate() {
            index_of.entry(feature.0).or_insert(index);
        }
        let mut by_id = HashMap::with_capacity(groups.len());
        let mut member_of = HashMap::new();
        let mut children: HashMap<u64, Vec<usize>> = HashMap::new();
        for (index, group) in groups.iter().enumerate() {
            by_id.entry(group.id).or_insert(index);
            for feature in &group.features {
                member_of.entry(*feature).or_insert(group.id);
            }
            if let Some(parent) = group.parent {
                children.entry(parent).or_default().push(index);
            }
        }
        let group_of: Vec<Option<u64>> = features.iter().map(|feature| member_of.get(&feature.0).copied()).collect();
        // `doc.sketch(id)` reads the first feature with the id.
        let has_patterns = features
            .iter()
            .map(|feature| {
                index_of.get(&feature.0).and_then(|index| doc.features.get(*index)).is_some_and(
                    |feature| matches!(&feature.kind, solidcraft_engine::doc::FeatureKind::Sketch { sketch, .. } if !sketch.patterns.is_empty() || sketch.links.iter().any(crate::sketch_link_tree::listed)),
                )
            })
            .collect();
        // A parent that is no folder (any more) is still looked up by its key, as before.
        let fold_ids: BTreeSet<u64> = groups.iter().flat_map(|group| [Some(group.id), group.parent]).flatten().collect();
        let mut plan = Self {
            key: PlanKey::of(app, under_feature),
            index_of,
            group_of,
            has_patterns,
            by_id,
            depth: Vec::new(),
            first: Vec::new(),
            position: Vec::new(),
            end: Vec::new(),
            starting: vec![Vec::new(); features.len()],
            ending: vec![Vec::new(); features.len()],
            trailing: Vec::new(),
            contents: Vec::new(),
            joints: Vec::new(),
            fold_keys: fold_ids.into_iter().map(|id| (id, format!("timeline_group:{id}"))).collect(),
            host_keys: rows.rows.iter().filter(|row| !row.listed.is_empty()).map(|row| (row.feature, sketches_key(row.feature))).collect(),
            features,
            groups,
            rows,
        };
        plan.place_folders(&children);
        plan
    }

    /// The folders' depths, extents, header places and contents.
    fn place_folders(&mut self, children: &HashMap<u64, Vec<usize>>) {
        let groups = &self.groups;
        let n = self.features.len();
        let ids: Vec<u64> = groups.iter().map(|group| group.id).collect();
        self.depth = ids.iter().map(|id| self.depth_of(*id)).collect();
        let mut first = HashMap::new();
        let mut last = HashMap::new();
        for id in &ids {
            self.extent(*id, children, &mut first, &mut last, &mut HashSet::new());
        }
        self.first = ids.iter().map(|id| first.get(id).copied().flatten()).collect();
        self.end = ids.iter().map(|id| last.get(id).copied().flatten().map_or(n, |index: usize| index.saturating_add(1))).collect();
        // A folder with only joints shares its nearest feature-bearing ancestor's position.
        self.position = ids
            .iter()
            .map(|id| {
                let mut current = Some(*id);
                for _ in 0..groups.len() {
                    let id = current?;
                    if let Some(first) = first.get(&id).copied().flatten() {
                        return Some(first);
                    }
                    current = self.parent_of(id);
                }
                None
            })
            .collect();
        let paths: Vec<Vec<u64>> = ids.iter().map(|id| group_path(groups, *id)).collect();
        for (index, position) in self.position.iter().enumerate() {
            if let Some(slot) = position.and_then(|position| self.starting.get_mut(position)) {
                slot.push(index);
            }
        }
        for starting in &mut self.starting {
            starting.sort_by(|a, b| paths.get(*a).cmp(&paths.get(*b)));
        }
        for (index, id) in ids.iter().enumerate() {
            if let Some(slot) = last.get(id).copied().flatten().and_then(|last| self.ending.get_mut(last)) {
                slot.push(index);
            }
        }
        for ending in &mut self.ending {
            ending.sort_by_key(|index| std::cmp::Reverse(self.depth.get(*index).copied()));
        }
        let mut trailing: Vec<usize> = (0..groups.len()).collect();
        let place = |at: usize| (self.position.get(at).copied().flatten().unwrap_or(usize::MAX), paths.get(at));
        trailing.sort_by(|a, b| place(*a).cmp(&place(*b)));
        self.trailing = trailing;
        // A folder holds its own features and those of the folders nested in it, as
        // `group_is_descendant` decides (a broken chain of parents counts as inside everything).
        let ancestors: HashMap<u64, Option<HashSet<u64>>> = ids.iter().map(|id| (*id, self.ancestors(*id))).collect();
        let inside = |group: u64, root: u64| group == root || ancestors.get(&group).is_some_and(|up| up.as_ref().is_none_or(|up| up.contains(&root)));
        self.contents = ids
            .iter()
            .map(|root| {
                self.group_of
                    .iter()
                    .zip(&self.features)
                    .filter(|(group, _)| group.is_some_and(|group| inside(group, *root)))
                    .map(|(_, feature)| feature.0)
                    .collect()
            })
            .collect();
        self.joints = ids
            .iter()
            .map(|root| groups.iter().filter(|candidate| inside(candidate.id, *root)).flat_map(|group| group.joints.iter().copied()).collect())
            .collect();
    }

    /// The first and last timeline positions of `id`'s own and nested features.
    fn extent(
        &self,
        id: u64,
        children: &HashMap<u64, Vec<usize>>,
        first: &mut HashMap<u64, Option<usize>>,
        last: &mut HashMap<u64, Option<usize>>,
        visiting: &mut HashSet<u64>,
    ) -> (Option<usize>, Option<usize>) {
        if let (Some(a), Some(b)) = (first.get(&id), last.get(&id)) {
            return (*a, *b);
        }
        // A folder nested in itself adds nothing more.
        if !visiting.insert(id) {
            return (None, None);
        }
        let Some(group) = self.by_id.get(&id).and_then(|index| self.groups.get(*index)) else {
            first.insert(id, None);
            last.insert(id, None);
            return (None, None);
        };
        let direct: Vec<usize> = group.features.iter().filter_map(|feature| self.index_of.get(feature).copied()).collect();
        let (mut low, mut high) = (direct.iter().copied().min(), direct.iter().copied().max());
        for child in children.get(&id).into_iter().flatten() {
            let Some(child) = self.groups.get(*child).map(|child| child.id) else { continue };
            let (a, b) = self.extent(child, children, first, last, visiting);
            low = low.into_iter().chain(a).min();
            high = high.into_iter().chain(b).max();
        }
        visiting.remove(&id);
        first.insert(id, low);
        last.insert(id, high);
        (low, high)
    }

    fn parent_of(&self, id: u64) -> Option<u64> {
        self.by_id.get(&id).and_then(|index| self.groups.get(*index)).and_then(|group| group.parent)
    }

    /// The depth of the first folder with `id` (`group_depth`).
    pub(super) fn depth_of(&self, id: u64) -> usize {
        let mut depth = 0;
        let mut current = self.parent_of(id);
        while let Some(parent) = current {
            depth += 1;
            if depth > self.groups.len() {
                break;
            }
            current = self.parent_of(parent);
        }
        depth
    }

    /// The folders above `id`; `None` when its chain of parents never ends.
    fn ancestors(&self, id: u64) -> Option<HashSet<u64>> {
        let mut found = HashSet::new();
        let mut current = self.parent_of(id);
        let mut steps = 0usize;
        while let Some(parent) = current {
            found.insert(parent);
            steps += 1;
            if steps > self.groups.len() {
                return None;
            }
            current = self.parent_of(parent);
        }
        Some(found)
    }

    /// Whether `id` and every folder around it are open (`group_is_open`).
    pub(super) fn is_open(&self, folds: &Folds, id: u64) -> bool {
        !folds.is_closed(id) && self.ancestors_open(folds, id)
    }

    /// Whether every folder around `id` is open (`group_ancestors_are_open`).
    pub(super) fn ancestors_open(&self, folds: &Folds, id: u64) -> bool {
        let mut current = self.parent_of(id);
        let mut steps = 0usize;
        while let Some(group) = current {
            if folds.is_closed(group) {
                return false;
            }
            steps += 1;
            if steps > self.groups.len() {
                return false;
            }
            current = self.parent_of(group);
        }
        true
    }

    /// Whether the feature with `id` is suppressed.
    pub(super) fn suppressed(&self, id: u64) -> bool {
        self.index_of.get(&id).and_then(|index| self.features.get(*index)).is_some_and(|feature| feature.3)
    }

    /// The name of the feature with `id`.
    pub(super) fn name(&self, id: u64) -> Option<&str> {
        self.index_of.get(&id).and_then(|index| self.features.get(*index)).map(|feature| feature.1.as_str())
    }

    /// Which folders and folds are open now.
    pub(super) fn folds(&self, app: &SolveApp) -> Folds {
        let closed = self.fold_keys.iter().filter(|(_, key)| app.tree.collapsed.contains(key)).map(|(id, _)| *id).collect();
        let open_hosts = self.host_keys.iter().filter(|(_, key)| crate::browser::is_open(app, key, false)).map(|(feature, _)| *feature).collect();
        Folds { closed, open_hosts }
    }

    /// The visible items top to bottom (`visible_order`), kept until a folder or fold opens or
    /// closes.
    pub(super) fn order(&self, ui: &egui::Ui, folds: &Folds) -> Arc<Vec<TimelineItem>> {
        type Kept = (PlanKey, Folds, Arc<Vec<TimelineItem>>);
        let id = egui::Id::new("sc_history_row_order");
        if let Some(kept) = ui.data(|data| data.get_temp::<Arc<Kept>>(id)).filter(|kept| kept.0 == self.key && kept.1 == *folds) {
            return kept.2.clone();
        }
        let order = Arc::new(self.visible_order(folds));
        ui.data_mut(|data| data.insert_temp(id, Arc::new((self.key, folds.clone(), order.clone()))));
        order
    }

    pub(super) fn visible_order(&self, folds: &Folds) -> Vec<TimelineItem> {
        let mut order = Vec::new();
        let mut shown = HashSet::new();
        for (index, (id, ..)) in self.features.iter().enumerate() {
            for group in self.starting.get(index).into_iter().flatten().filter_map(|group| self.groups.get(*group)) {
                if self.ancestors_open(folds, group.id) && shown.insert(group.id) {
                    order.push(TimelineItem::Group(group.id));
                }
            }
            // A listed sketch follows the feature built from it.
            let Some(row) = self.rows.row_at(index) else { continue };
            if self.group_of.get(index).copied().flatten().is_none_or(|group| self.is_open(folds, group)) {
                order.push(TimelineItem::Feature(*id));
                if !row.listed.is_empty() && folds.open_hosts.contains(id) {
                    order.extend(row.listed.iter().filter(|listed| !listed.reference).map(|listed| TimelineItem::Feature(listed.sketch)));
                }
            }
        }
        for group in self.trailing.iter().filter_map(|group| self.groups.get(*group)) {
            if !shown.contains(&group.id) && self.ancestors_open(folds, group.id) && shown.insert(group.id) {
                order.push(TimelineItem::Group(group.id));
            }
        }
        order
    }
}

#[cfg(test)]
impl RowPlan {
    /// Everything the plan holds, comparable: a cached plan must equal one built afresh.
    pub(super) fn summary(&self) -> String {
        let features: Vec<_> = self.features.iter().map(|f| (f.0, &f.1, f.2, f.3, f.4, std::mem::discriminant(&f.5))).collect();
        let mut index_of: Vec<_> = self.index_of.iter().collect();
        index_of.sort();
        let mut by_id: Vec<_> = self.by_id.iter().collect();
        by_id.sort();
        format!(
            "{features:?}\n{:?}\n{:?}\n{index_of:?}\n{:?}\n{:?}\n{by_id:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}",
            self.groups,
            self.rows,
            self.group_of,
            self.has_patterns,
            self.depth,
            self.first,
            self.position,
            self.end,
            self.starting,
            self.ending,
            self.trailing,
            self.contents,
            self.joints,
            self.fold_keys,
            self.host_keys,
        )
    }

    pub(super) fn under_feature(&self) -> bool {
        self.key.under_feature
    }

    /// The plan the History drew with last, from the egui context.
    pub(super) fn kept(ctx: &egui::Context) -> Option<Arc<Self>> {
        ctx.data(|data| data.get_temp::<Arc<Self>>(egui::Id::new("sc_history_row_plan")))
    }
}
