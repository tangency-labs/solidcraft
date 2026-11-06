// SPDX-License-Identifier: Apache-2.0
use super::Slots;
use crate::history_records::{
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCurveAxis,
    AsmHistoricalCylinder, AsmHistoricalEdge, AsmHistoricalEdgeSample,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPersistentSubentityTag, AsmHistoricalPlane,
    AsmHistoricalPoint, AsmHistoricalRelation, AsmHistoricalSurfaceAxis,
    AsmHistoricalSurfaceRadius, AsmHistoricalTopology,
};
use std::collections::BTreeMap;

/// One bit per topology family.
type Families = u64;

fn group<T>(
    rows: Vec<T>,
    key: impl Fn(&T) -> i64,
    family: Families,
    families: &mut BTreeMap<i64, Families>,
) -> BTreeMap<i64, Vec<T>> {
    let mut groups = BTreeMap::<i64, Vec<T>>::new();
    for row in rows {
        let slot = key(&row);
        *families.entry(slot).or_default() |= family;
        groups.entry(slot).or_default().push(row);
    }
    groups
}

fn ordered_rows<T>(rows: &[T], key: impl Fn(&T) -> i64) -> bool {
    rows.windows(2).all(|pair| key(&pair[0]) <= key(&pair[1]))
}

/// One replaced key: the families it held before the replacement and the
/// families the replacement has written.
struct KeyEdit {
    key: i64,
    present: Families,
    written: Families,
}

/// Overwrite the rows of each edited key in place. A key's first run clears
/// its old rows and later runs append, so an unsorted part keeps every row.
fn write_family<T: Clone>(
    current: &mut BTreeMap<i64, Vec<T>>,
    edits: &mut [KeyEdit],
    rows: &[T],
    key: impl Fn(&T) -> i64,
    family: Families,
) {
    for run in rows.chunk_by(|left, right| key(left) == key(right)) {
        let slot = key(&run[0]);
        let Ok(at) = edits.binary_search_by_key(&slot, |edit| edit.key) else {
            continue;
        };
        let target = current.entry(slot).or_default();
        if edits[at].written & family == 0 {
            target.clear();
            edits[at].written |= family;
        }
        target.extend_from_slice(run);
    }
}

macro_rules! indexed_topology {
    (entities { $($entity:ident = $ebit:literal: $ety:ty => $ekey:expr),* $(,)? }
     containers { $($container:ident = $cbit:literal: $cty:ty => $ckey:expr),* $(,)? }) => {
        /// Rows of each family grouped by key. `families` holds the families
        /// with rows at each slot, so a keyed edit visits only those.
        #[derive(Default)]
        pub(super) struct IndexedTopology {
            families: BTreeMap<i64, Families>,
            $($entity: BTreeMap<i64, Vec<$ety>>),*,
            $($container: BTreeMap<i64, Vec<$cty>>),*
        }
        impl IndexedTopology {
            const ENTITIES: Families = $((1 << $ebit))|*;
            const CONTAINERS: Families = $((1 << $cbit))|*;
        }
        const _: () = assert!(
            IndexedTopology::ENTITIES | IndexedTopology::CONTAINERS
                == (1 << [$($ebit,)* $($cbit,)*].len()) - 1,
            "family bits are distinct and dense"
        );
        impl From<AsmHistoricalTopology> for IndexedTopology {
            fn from(topology: AsmHistoricalTopology) -> Self {
                let mut families = BTreeMap::new();
                $(let $entity = group(topology.$entity, $ekey, 1 << $ebit, &mut families);)*
                $(let $container = group(topology.$container, $ckey, 1 << $cbit, &mut families);)*
                Self { families, $($entity),*, $($container),* }
            }
        }
        impl IndexedTopology {
            pub(super) fn ordered(topology: &AsmHistoricalTopology) -> bool {
                $(ordered_rows(&topology.$entity, $ekey))&&*
                    && $(ordered_rows(&topology.$container, $ckey))&&*
            }
            pub(super) fn bodies(&self) -> impl Iterator<Item = &i64> { self.bodies.keys() }
            pub(super) fn materialize(&self) -> AsmHistoricalTopology {
                AsmHistoricalTopology {
                    $($entity: self.$entity.values().flatten().cloned().collect()),*,
                    $($container: self.$container.values().flatten().cloned().collect()),*
                }
            }
            pub(super) fn project(&self, keys: &Slots, containers: bool) -> AsmHistoricalTopology {
                let mut out = AsmHistoricalTopology::default();
                for key in keys {
                    let Some(&present) = self.families.get(key) else {
                        continue;
                    };
                    if containers {
                        $(if present & (1 << $cbit) != 0 {
                            out.$container.extend(self.$container.get(key).into_iter().flatten().cloned());
                        })*
                    } else {
                        $(if present & (1 << $ebit) != 0 {
                            out.$entity.extend(self.$entity.get(key).into_iter().flatten().cloned());
                        })*
                    }
                }
                out
            }
            pub(super) fn replace(&mut self, part: AsmHistoricalTopology, keys: &Slots, containers: bool) {
                let half = if containers { Self::CONTAINERS } else { Self::ENTITIES };
                let mut edits = keys
                    .iter()
                    .map(|&key| KeyEdit {
                        key,
                        present: self.families.get(&key).copied().unwrap_or_default(),
                        written: 0,
                    })
                    .collect::<Vec<_>>();
                if containers {
                    $(write_family(&mut self.$container, &mut edits, &part.$container, $ckey, 1 << $cbit);)*
                } else {
                    $(write_family(&mut self.$entity, &mut edits, &part.$entity, $ekey, 1 << $ebit);)*
                }
                for edit in edits {
                    let stale = edit.present & half & !edit.written;
                    if containers {
                        $(if stale & (1 << $cbit) != 0 {
                            self.$container.remove(&edit.key);
                        })*
                    } else {
                        $(if stale & (1 << $ebit) != 0 {
                            self.$entity.remove(&edit.key);
                        })*
                    }
                    let present = (edit.present & !half) | edit.written;
                    if present == edit.present {
                        continue;
                    }
                    if present == 0 {
                        self.families.remove(&edit.key);
                    } else {
                        self.families.insert(edit.key, present);
                    }
                }
            }
        }
    };
}

indexed_topology! {
    entities {
        faces = 0: i64 => |row: &i64| *row,
        loops = 1: i64 => |row: &i64| *row,
        coedges = 2: i64 => |row: &i64| *row,
        edges = 3: i64 => |row: &i64| *row,
        vertices = 4: i64 => |row: &i64| *row,
        points = 5: i64 => |row: &i64| *row,
        surfaces = 6: i64 => |row: &i64| *row,
        surface_radii = 7: AsmHistoricalSurfaceRadius => |row: &AsmHistoricalSurfaceRadius| row.surface,
        surface_cylinders = 8: AsmHistoricalCylinder => |row: &AsmHistoricalCylinder| row.surface,
        surface_planes = 9: AsmHistoricalPlane => |row: &AsmHistoricalPlane| row.surface,
        surface_axes = 10: AsmHistoricalSurfaceAxis => |row: &AsmHistoricalSurfaceAxis| row.surface,
        curves = 11: i64 => |row: &i64| *row,
        curve_axes = 12: AsmHistoricalCurveAxis => |row: &AsmHistoricalCurveAxis| row.curve,
        pcurves = 13: i64 => |row: &i64| *row,
        persistent_subentity_tags = 14: AsmHistoricalPersistentSubentityTag => |row: &AsmHistoricalPersistentSubentityTag| row.entity_ref,
        face_loops = 15: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        loop_coedges = 16: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        coedge_topology = 17: AsmHistoricalCoedge => |row: &AsmHistoricalCoedge| row.coedge,
        edge_vertices = 18: AsmHistoricalEdge => |row: &AsmHistoricalEdge| row.edge,
        face_surfaces = 19: AsmHistoricalCarrierBinding => |row: &AsmHistoricalCarrierBinding| row.entity,
        edge_curves = 20: AsmHistoricalOptionalCarrierBinding => |row: &AsmHistoricalOptionalCarrierBinding| row.entity,
        coedge_pcurves = 21: AsmHistoricalOptionalCarrierBinding => |row: &AsmHistoricalOptionalCarrierBinding| row.entity,
        vertex_points = 22: AsmHistoricalCarrierBinding => |row: &AsmHistoricalCarrierBinding| row.entity,
        point_positions = 23: AsmHistoricalPoint => |row: &AsmHistoricalPoint| row.point,
        edge_samples = 24: AsmHistoricalEdgeSample => |row: &AsmHistoricalEdgeSample| row.edge,
    }
    containers {
        bodies = 25: i64 => |row: &i64| *row,
        regions = 26: i64 => |row: &i64| *row,
        shells = 27: i64 => |row: &i64| *row,
        body_regions = 28: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        region_shells = 29: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        shell_faces = 30: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        shell_wire_edges = 31: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
        shell_free_vertices = 32: AsmHistoricalRelation => |row: &AsmHistoricalRelation| row.owner_ref,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_records::{AsmHistoricalTransition, AsmStateSnapshot};
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    #[test]
    fn keyed_chain_matches_full_diffs_with_repeats_and_tag_groups() {
        let tag = |ordinal| AsmHistoricalPersistentSubentityTag {
            entity_kind: AsmHistoricalEntityKind::Face,
            entity_ref: 1,
            selector: 7,
            token: "face".into(),
            design_references: vec![8],
            ordinal,
        };
        let mut newer = AsmHistoricalTopology {
            faces: vec![1],
            surfaces: vec![10, 10, 20],
            curves: vec![40, 40],
            persistent_subentity_tags: vec![tag(0), tag(1)],
            ..Default::default()
        };
        let mut indexed = IndexedTopology::from(newer.clone());
        let mut newer_versions = BTreeMap::from([(1, 1), (10, 10), (20, 20), (40, 40)]);
        let keys = Slots::from([1, 10, 40]);
        for state_id in [2, 1] {
            let mut older = newer.clone();
            older.surfaces.retain(|slot| *slot != 10);
            older.surfaces.insert(0, 10);
            older.curves.clear();
            older.persistent_subentity_tags = vec![tag(2)];
            let mut older_versions = newer_versions.clone();
            for slot in &keys {
                older_versions.insert(*slot, *slot + state_id * 100);
            }
            let newer_part = indexed.project(&keys, false);
            indexed.replace(older.clone(), &keys, false);
            let older_part = indexed.project(&keys, false);
            assert_eq!(indexed.materialize(), older);
            let old = AsmStateSnapshot {
                state_id,
                topology: &older,
                versions: &older_versions,
            };
            let new = AsmStateSnapshot {
                state_id: state_id + 1,
                topology: &newer,
                versions: &newer_versions,
            };
            assert_eq!(
                super::super::transition_for_changes(
                    AsmStateSnapshot {
                        topology: &older_part,
                        ..old
                    },
                    AsmStateSnapshot {
                        topology: &newer_part,
                        ..new
                    },
                    &keys,
                ),
                AsmHistoricalTransition::between(Some(old), new),
            );
            newer = older;
            newer_versions = older_versions;
        }
    }
    #[test]
    fn sparse_edit_preserves_other_slots() {
        let mut indexed = IndexedTopology::from(AsmHistoricalTopology {
            surfaces: vec![10, 20, 30],
            ..Default::default()
        });
        let keys = Slots::from([20]);
        assert_eq!(indexed.project(&keys, false).surfaces, vec![20]);
        indexed.replace(
            AsmHistoricalTopology {
                surfaces: vec![20, 20],
                ..Default::default()
            },
            &keys,
            false,
        );
        assert_eq!(indexed.materialize().surfaces, vec![10, 20, 20, 30]);
        assert_eq!(indexed.project(&keys, false).surfaces, vec![20, 20]);
    }

    #[test]
    fn replace_keeps_every_row_of_an_unsorted_part() {
        let mut indexed = IndexedTopology::from(AsmHistoricalTopology {
            surfaces: vec![10, 20, 30],
            ..Default::default()
        });
        indexed.replace(
            AsmHistoricalTopology {
                surfaces: vec![20, 10, 20],
                ..Default::default()
            },
            &Slots::from([10, 20]),
            false,
        );
        assert_eq!(indexed.materialize().surfaces, vec![10, 20, 20, 30]);
    }

    #[test]
    fn replace_keeps_the_other_half_of_a_slot() {
        let mut indexed = IndexedTopology::from(AsmHistoricalTopology {
            faces: vec![5],
            surfaces: vec![5],
            bodies: vec![5],
            ..Default::default()
        });
        let keys = Slots::from([5]);
        indexed.replace(
            AsmHistoricalTopology {
                surfaces: vec![5],
                ..Default::default()
            },
            &keys,
            false,
        );
        let replaced = indexed.materialize();
        assert!(replaced.faces.is_empty());
        assert_eq!(replaced.surfaces, vec![5]);
        assert_eq!(replaced.bodies, vec![5]);
        indexed.replace(AsmHistoricalTopology::default(), &keys, true);
        assert!(indexed.project(&keys, true).bodies.is_empty());
        assert_eq!(indexed.project(&keys, false).surfaces, vec![5]);
    }
}
