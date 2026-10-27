//! The rows of the feature History with sketches listed under the features built from them
//! (#6489): which feature each sketch is listed under, which later features point back at a sketch
//! listed under an earlier one, and which timeline position a drop above each row stands for.
//!
//! Plain data with no UI in it, so every place that draws the History lists sketches the same
//! way: the vertical History (`timeline_left`) draws exactly these rows, and another host (a
//! History in the browser) can draw them and map its drop slots from the same rows.

use std::collections::{BTreeMap, BTreeSet};

use solidcraft_engine::doc::{Feature, FeatureKind, TimelineGroup};

/// A timeline feature as the History needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryFeature {
    pub id: u64,
    /// The feature is a sketch.
    pub sketch: bool,
    /// The sketches the feature is built from (`FeatureKind::input_sketches`: profiles, paths,
    /// points). Empty for a sketch: a sketch is never listed under another sketch.
    pub inputs: Vec<u64>,
}

impl HistoryFeature {
    pub(crate) fn of(feature: &Feature) -> Self {
        let sketch = matches!(feature.kind, FeatureKind::Sketch { .. });
        Self { id: feature.id, sketch, inputs: if sketch { Vec::new() } else { feature.kind.input_sketches() } }
    }
}

/// A sketch row one level in, under the feature built from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ListedSketch {
    pub sketch: u64,
    /// The sketch's position in the timeline.
    pub index: usize,
    /// The sketch is listed under an earlier feature, and this row only points at it: it selects
    /// that sketch, isn't dragged and isn't a drop slot.
    pub reference: bool,
}

/// A row of its own, in timeline order: a feature with the sketches listed under it, or a sketch
/// no feature lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HistoryRow {
    pub feature: u64,
    /// The feature's position in the timeline.
    pub index: usize,
    /// The row is a sketch (one that no feature lists).
    pub sketch: bool,
    /// The timeline position a drop just above the row stands for: where a moved feature goes and
    /// where the history marker rolls to. It comes before the sketches listed under the row that
    /// follow the previous row (or folder header, or marker), so they are moved or rolled back with
    /// it. A drop below the last row stands for the end of the timeline.
    pub slot: usize,
    /// The sketches listed one level in, in timeline order.
    pub listed: Vec<ListedSketch>,
}

/// The History's rows, in timeline order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HistoryRows {
    pub rows: Vec<HistoryRow>,
    /// Sketch → the feature it is listed under (not its reference rows).
    hosts: BTreeMap<u64, u64>,
}

impl HistoryRows {
    /// Every feature and sketch a row of its own, in creation order: `(id, is a sketch)` for each
    /// feature in timeline order.
    pub(crate) fn in_order(features: impl IntoIterator<Item = (u64, bool)>) -> Self {
        let rows = features
            .into_iter()
            .enumerate()
            .map(|(index, (feature, sketch))| HistoryRow { feature, index, sketch, slot: index, listed: Vec::new() })
            .collect();
        Self { rows, hosts: BTreeMap::new() }
    }

    /// The feature `sketch` is listed under, when it is.
    pub(crate) fn host(&self, sketch: u64) -> Option<u64> {
        self.hosts.get(&sketch).copied()
    }

    /// The row of the feature at timeline position `index`, when it has a row of its own.
    pub(crate) fn row_at(&self, index: usize) -> Option<&HistoryRow> {
        self.rows.binary_search_by_key(&index, |row| row.index).ok().and_then(|at| self.rows.get(at))
    }

    /// Whether a reference row points at `sketch`.
    pub(crate) fn referenced(&self, sketch: u64) -> bool {
        self.rows.iter().flat_map(|row| &row.listed).any(|listed| listed.reference && listed.sketch == sketch)
    }

    /// The sketches listed under `feature` (not its reference rows): they move with it.
    pub(crate) fn listed_under(&self, feature: u64) -> impl Iterator<Item = u64> + '_ {
        self.hosts.iter().filter(move |(_, host)| **host == feature).map(|(sketch, _)| *sketch)
    }

    /// `features` and the sketches listed under them, which a move takes along.
    pub(crate) fn with_listed(&self, features: &[u64]) -> Vec<u64> {
        let mut all = features.to_vec();
        for sketch in features.iter().flat_map(|feature| self.listed_under(*feature)) {
            if !all.contains(&sketch) {
                all.push(sketch);
            }
        }
        all
    }
}

/// The History's rows with each sketch listed under the first feature built from it.
///
/// - A sketch is listed one level in under the first feature built from it when that feature is
///   not a sketch, comes after it, is in the same timeline folder and is on the same side of the
///   history marker (`marker`: the features at or after it are rolled back; `None` for none). With
///   the marker between them the sketch exists and the feature doesn't, so the sketch keeps a row
///   of its own. A sketch no feature uses, or one its first feature can't list, keeps its row too.
/// - With `reference_rows`, each later feature built from a listed sketch also lists it, as a
///   reference row.
pub(crate) fn rows(features: &[HistoryFeature], folders: &[TimelineGroup], marker: Option<usize>, reference_rows: bool) -> HistoryRows {
    let marker = marker.unwrap_or(features.len());
    let sketches: BTreeMap<u64, usize> =
        features.iter().enumerate().filter(|(_, feature)| feature.sketch).map(|(index, feature)| (feature.id, index)).collect();
    // A feature's folder: the first folder that holds it, as the timeline finds it.
    let mut folder_of = BTreeMap::new();
    for folder in folders {
        for feature in &folder.features {
            folder_of.entry(*feature).or_insert(folder.id);
        }
    }
    let mut hosts = BTreeMap::new();
    let mut listed: BTreeMap<usize, Vec<ListedSketch>> = BTreeMap::new();
    // Sketches whose first feature has been met, listed or not.
    let mut used = BTreeSet::new();
    for (index, feature) in features.iter().enumerate() {
        if feature.sketch {
            continue;
        }
        let mut inputs: Vec<(usize, u64)> =
            feature.inputs.iter().filter_map(|sketch| sketches.get(sketch).map(|at| (*at, *sketch))).filter(|(at, _)| *at < index).collect();
        inputs.sort_unstable();
        inputs.dedup();
        for (at, sketch) in inputs {
            let reference = if used.insert(sketch) {
                let marker_between = at < marker && marker <= index;
                if marker_between || folder_of.get(&sketch) != folder_of.get(&feature.id) {
                    continue;
                }
                hosts.insert(sketch, feature.id);
                false
            } else if reference_rows && hosts.contains_key(&sketch) {
                true
            } else {
                continue;
            };
            listed.entry(index).or_default().push(ListedSketch { sketch, index: at, reference });
        }
    }
    // A drop above a row comes before the listed sketches that follow the previous row; a folder
    // header and the marker are drawn at their own positions, so the slot after them starts there.
    let starts = folder_starts(features, folders);
    let mut rows = Vec::new();
    let mut slot = 0;
    for (index, feature) in features.iter().enumerate() {
        if index == marker || starts.contains(&index) {
            slot = index;
        }
        if hosts.contains_key(&feature.id) {
            continue;
        }
        rows.push(HistoryRow { feature: feature.id, index, sketch: feature.sketch, slot, listed: listed.remove(&index).unwrap_or_default() });
        slot = index + 1;
    }
    HistoryRows { rows, hosts }
}

/// The timeline positions where a folder's header is drawn: before its first feature, its own or
/// a nested folder's.
fn folder_starts(features: &[HistoryFeature], folders: &[TimelineGroup]) -> BTreeSet<usize> {
    let positions: BTreeMap<u64, usize> = features.iter().enumerate().map(|(index, feature)| (feature.id, index)).collect();
    let mut first: BTreeMap<u64, usize> = BTreeMap::new();
    for folder in folders {
        for at in folder.features.iter().filter_map(|feature| positions.get(feature)) {
            // The folder and each one above it; a hand-edited cycle stops after every folder.
            let mut current = Some(folder.id);
            for _ in 0..=folders.len() {
                let Some(id) = current else { break };
                let start = first.entry(id).or_insert(*at);
                *start = (*start).min(*at);
                current = folders.iter().find(|candidate| candidate.id == id).and_then(|candidate| candidate.parent);
            }
        }
    }
    first.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sketch(id: u64) -> HistoryFeature {
        HistoryFeature { id, sketch: true, inputs: Vec::new() }
    }

    fn built(id: u64, inputs: &[u64]) -> HistoryFeature {
        HistoryFeature { id, sketch: false, inputs: inputs.to_vec() }
    }

    fn folder(id: u64, features: &[u64], parent: Option<u64>) -> TimelineGroup {
        TimelineGroup { id, name: format!("Folder{id}"), parent, features: features.to_vec() }
    }

    /// The top-level rows as `(feature, slot, listed sketches as (sketch, reference))`.
    fn shape(rows: &HistoryRows) -> Vec<(u64, usize, Vec<(u64, bool)>)> {
        rows.rows.iter().map(|row| (row.feature, row.slot, row.listed.iter().map(|listed| (listed.sketch, listed.reference)).collect())).collect()
    }

    #[test]
    fn in_order_gives_every_feature_a_row_at_its_own_position() {
        let rows = HistoryRows::in_order([(1, true), (2, false), (3, true)]);
        assert_eq!(shape(&rows), vec![(1, 0, vec![]), (2, 1, vec![]), (3, 2, vec![])]);
        assert_eq!(rows.rows.iter().map(|row| row.sketch).collect::<Vec<_>>(), vec![true, false, true]);
        assert_eq!(rows.host(1), None);
        assert_eq!(rows.row_at(1).map(|row| row.feature), Some(2));
        assert_eq!(rows.with_listed(&[2]), vec![2]);
    }

    #[test]
    fn a_sketch_is_listed_under_the_first_feature_built_from_it() {
        // Profile → Block, Pocket → Second, and Unused, which no feature uses.
        let features = [sketch(1), built(2, &[1]), sketch(3), built(4, &[3]), sketch(5)];
        let rows = rows(&features, &[], None, true);
        assert_eq!(shape(&rows), vec![(2, 0, vec![(1, false)]), (4, 2, vec![(3, false)]), (5, 4, vec![])]);
        assert_eq!(rows.rows.last().map(|row| row.sketch), Some(true), "a sketch no feature uses is a row of its own");
        assert_eq!((rows.host(1), rows.host(3), rows.host(5)), (Some(2), Some(4), None));
        assert_eq!(rows.row_at(0), None, "a listed sketch has no row of its own");
        assert_eq!(rows.rows.first().and_then(|row| row.listed.first()).map(|listed| listed.index), Some(0));
        assert_eq!(rows.with_listed(&[4, 2]), vec![4, 2, 3, 1], "a move takes the listed sketches along");
    }

    #[test]
    fn later_features_point_at_a_shared_sketch_only_with_reference_rows() {
        let features = [sketch(1), built(2, &[1]), built(3, &[1])];
        let with = rows(&features, &[], None, true);
        assert_eq!(shape(&with), vec![(2, 0, vec![(1, false)]), (3, 2, vec![(1, true)])]);
        assert!(with.referenced(1));
        assert_eq!(with.with_listed(&[3]), vec![3], "a reference row doesn't move the sketch");
        assert_eq!(with.listed_under(3).count(), 0);
        let without = rows(&features, &[], None, false);
        assert_eq!(shape(&without), vec![(2, 0, vec![(1, false)]), (3, 2, vec![])]);
        assert!(!without.referenced(1));
    }

    #[test]
    fn the_marker_between_a_sketch_and_its_feature_keeps_the_sketch_a_row_of_its_own() {
        let features = [sketch(1), built(2, &[1]), built(3, &[1])];
        // The sketch exists, its features don't: no row lists it, not even as a reference.
        assert_eq!(shape(&rows(&features, &[], Some(1), true)), vec![(1, 0, vec![]), (2, 1, vec![]), (3, 2, vec![])]);
        // Both rolled back, or both before the marker: listed.
        assert_eq!(shape(&rows(&features, &[], Some(0), true)), vec![(2, 0, vec![(1, false)]), (3, 2, vec![(1, true)])]);
        assert_eq!(shape(&rows(&features, &[], Some(2), true)), vec![(2, 0, vec![(1, false)]), (3, 2, vec![(1, true)])]);
    }

    #[test]
    fn a_sketch_in_another_folder_than_its_feature_keeps_its_row() {
        let features = [sketch(1), built(2, &[1]), sketch(3), built(4, &[3])];
        let folders = [folder(10, &[1], None), folder(11, &[3, 4], None)];
        assert_eq!(shape(&rows(&features, &folders, None, true)), vec![(1, 0, vec![]), (2, 1, vec![]), (4, 2, vec![(3, false)])]);
        // Its later features don't list it either.
        let features = [sketch(1), built(2, &[1]), built(3, &[1])];
        assert_eq!(shape(&rows(&features, &[folder(10, &[2], None)], None, true)), vec![(1, 0, vec![]), (2, 1, vec![]), (3, 2, vec![])]);
    }

    #[test]
    fn a_drop_above_a_row_comes_before_the_sketches_listed_under_it() {
        // Sketches drawn ahead of their features, either way round.
        let features = [sketch(1), sketch(2), built(3, &[1]), built(4, &[2])];
        assert_eq!(shape(&rows(&features, &[], None, true)), vec![(3, 0, vec![(1, false)]), (4, 3, vec![(2, false)])]);
        let features = [sketch(2), sketch(1), built(3, &[1]), built(4, &[2])];
        assert_eq!(shape(&rows(&features, &[], None, true)), vec![(3, 0, vec![(1, false)]), (4, 3, vec![(2, false)])]);
        // A folder header between a sketch and its feature: the folder's rows start at the folder.
        let features = [sketch(1), built(2, &[]), built(3, &[]), built(4, &[1])];
        let folders = [folder(10, &[2, 3], None)];
        assert_eq!(shape(&rows(&features, &folders, None, true)), vec![(2, 1, vec![]), (3, 2, vec![]), (4, 3, vec![(1, false)])]);
        // A folder that starts with a listed sketch: the drop above its feature goes inside it.
        let features = [built(1, &[]), sketch(2), built(3, &[2])];
        let folders = [folder(10, &[2, 3], None)];
        assert_eq!(shape(&rows(&features, &folders, None, true)), vec![(1, 0, vec![]), (3, 1, vec![(2, false)])]);
        // A nested folder starts its parent too.
        let folders = [folder(10, &[], None), folder(11, &[2, 3], Some(10))];
        assert_eq!(folder_starts(&features, &folders), BTreeSet::from([1]));
    }

    #[test]
    fn several_sketches_and_odd_inputs_are_listed_once_in_timeline_order() {
        // A feature built from two sketches (a sweep's profile and path), named twice and out of
        // order, plus a sketch that comes after it and an id that isn't in the timeline.
        let features = [sketch(1), built(2, &[]), sketch(3), built(4, &[3, 1, 3, 5, 99]), sketch(5)];
        // The drop above the first row comes before the sketch listed under the later one.
        assert_eq!(shape(&rows(&features, &[], None, true)), vec![(2, 0, vec![]), (4, 2, vec![(1, false), (3, false)]), (5, 4, vec![])]);
        // A sketch never lists another sketch.
        let features = [sketch(1), HistoryFeature { id: 2, sketch: true, inputs: vec![1] }];
        assert_eq!(shape(&rows(&features, &[], None, true)), vec![(1, 0, vec![]), (2, 1, vec![])]);
    }

    #[test]
    fn features_read_the_sketches_they_are_built_from_out_of_the_design() {
        use serde_json::json;
        let mut session = solidcraft_engine::Session::default();
        for (command, params) in [
            ("sketch.create", json!({"plane": "XY", "name": "Profile"})),
            ("sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]})),
            ("sketch.finish", json!({})),
            ("solid.extrude", json!({"sketch": "Profile", "distance": 5, "operation": "new", "name": "Block"})),
            ("solid.extrude", json!({"sketch": "Profile", "distance": 5, "operation": "new", "name": "Copy"})),
        ] {
            session.execute(command, &params).unwrap();
        }
        let id = |name: &str| session.doc.find_feature(name).unwrap().id;
        let features: Vec<HistoryFeature> = session.doc.features.iter().map(HistoryFeature::of).collect();
        assert_eq!(
            features.iter().map(|feature| (feature.id, feature.sketch)).collect::<Vec<_>>(),
            vec![(id("Profile"), true), (id("Block"), false), (id("Copy"), false)]
        );
        assert!(features.iter().skip(1).all(|feature| !feature.inputs.is_empty() && feature.inputs.iter().all(|sketch| *sketch == id("Profile"))));
        let rows = rows(&features, &session.doc.timeline_groups, session.doc.marker, true);
        assert_eq!(rows.host(id("Profile")), Some(id("Block")));
        assert!(rows.referenced(id("Profile")));
    }

    #[test]
    fn hand_edited_folders_cannot_hang_it() {
        let features = [sketch(1), built(2, &[1])];
        let cycle = [folder(10, &[1, 2], Some(11)), folder(11, &[], Some(10)), folder(10, &[2], None)];
        assert_eq!(shape(&rows(&features, &cycle, None, true)), vec![(2, 0, vec![(1, false)])]);
        assert_eq!(shape(&rows(&[], &cycle, Some(7), true)), vec![]);
    }
}
