// SPDX-License-Identifier: Apache-2.0
//! Prove complete body membership from one immutable historical topology.
use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
use std::collections::{HashMap, HashSet};

fn occurrence_counts(slots: &[i64]) -> HashMap<i64, usize> {
    let mut counts = HashMap::new();
    for &slot in slots {
        *counts.entry(slot).or_default() += 1;
    }
    counts
}

struct RelationIndex<'a> {
    members_by_owner: HashMap<i64, Option<&'a [i64]>>,
    owner_by_member: HashMap<i64, Option<i64>>,
}

fn relation_index(relations: &[AsmHistoricalRelation]) -> RelationIndex<'_> {
    let mut members_by_owner = HashMap::new();
    let mut owner_by_member = HashMap::new();
    for relation in relations {
        members_by_owner
            .entry(relation.owner_ref)
            .and_modify(|members| *members = None)
            .or_insert(Some(relation.member_refs.as_slice()));
        for &member in &relation.member_refs {
            owner_by_member
                .entry(member)
                .and_modify(|owner| *owner = None)
                .or_insert(Some(relation.owner_ref));
        }
    }
    RelationIndex {
        members_by_owner,
        owner_by_member,
    }
}

pub(super) struct CompleteBodyIndex<'a> {
    body_counts: HashMap<i64, usize>,
    region_counts: HashMap<i64, usize>,
    shell_counts: HashMap<i64, usize>,
    face_counts: HashMap<i64, usize>,
    body_regions: RelationIndex<'a>,
    region_shells: RelationIndex<'a>,
    shell_faces: RelationIndex<'a>,
}
impl<'a> CompleteBodyIndex<'a> {
    pub(super) fn new(topology: &'a AsmHistoricalTopology) -> Self {
        let body_counts = occurrence_counts(&topology.bodies);
        let region_counts = occurrence_counts(&topology.regions);
        let shell_counts = occurrence_counts(&topology.shells);
        let face_counts = occurrence_counts(&topology.faces);
        let body_regions = relation_index(&topology.body_regions);
        let region_shells = relation_index(&topology.region_shells);
        let shell_faces = relation_index(&topology.shell_faces);

        Self {
            body_counts,
            region_counts,
            shell_counts,
            face_counts,
            body_regions,
            region_shells,
            shell_faces,
        }
    }
    pub(super) fn faces(&self, body: i64) -> Option<Vec<i64>> {
        macro_rules! complete_some {
            ($value:expr) => {
                match $value {
                    Some(value) => value,
                    None => return None,
                }
            };
        }
        let Self {
            body_counts,
            region_counts,
            shell_counts,
            face_counts,
            body_regions,
            region_shells,
            shell_faces,
        } = self;
        if body_counts.get(&body).copied() != Some(1) {
            return None;
        }
        let regions = complete_some!(body_regions.members_by_owner.get(&body).copied().flatten());
        if regions.is_empty() {
            return None;
        }
        let mut seen_regions = HashSet::new();
        let mut seen_shells = HashSet::new();
        let mut seen_faces = HashSet::new();
        for &region in regions {
            if !seen_regions.insert(region)
                || region_counts.get(&region).copied() != Some(1)
                || body_regions.owner_by_member.get(&region).copied().flatten() != Some(body)
            {
                return None;
            }
            let shells = complete_some!(region_shells
                .members_by_owner
                .get(&region)
                .copied()
                .flatten());
            if shells.is_empty() {
                return None;
            }
            for &shell in shells {
                if !seen_shells.insert(shell)
                    || shell_counts.get(&shell).copied() != Some(1)
                    || region_shells.owner_by_member.get(&shell).copied().flatten() != Some(region)
                {
                    return None;
                }
                let faces =
                    complete_some!(shell_faces.members_by_owner.get(&shell).copied().flatten());
                if faces.is_empty() {
                    return None;
                }
                for &face in faces {
                    if !seen_faces.insert(face)
                        || face_counts.get(&face).copied() != Some(1)
                        || shell_faces.owner_by_member.get(&face).copied().flatten() != Some(shell)
                    {
                        return None;
                    }
                }
            }
        }
        let mut faces = seen_faces.into_iter().collect::<Vec<_>>();
        faces.sort_unstable_by(Ord::cmp);
        (!faces.is_empty()).then_some(faces)
    }
}

#[cfg(test)]
mod tests {
    use super::CompleteBodyIndex;
    use crate::history::cache::SnapshotCache;
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    #[test]
    fn complete_body_queries_reuse_indexes_and_keep_snapshots_separate() {
        let topology = |face| AsmHistoricalTopology {
            bodies: vec![1],
            regions: vec![2],
            shells: vec![3],
            faces: vec![face],
            body_regions: vec![AsmHistoricalRelation {
                owner_ref: 1,
                member_refs: vec![2],
            }],
            region_shells: vec![AsmHistoricalRelation {
                owner_ref: 2,
                member_refs: vec![3],
            }],
            shell_faces: vec![AsmHistoricalRelation {
                owner_ref: 3,
                member_refs: vec![face],
            }],
            ..Default::default()
        };
        let first = topology(10);
        let second = topology(11);
        let mut cache = SnapshotCache::default();
        for _ in 0..1_000 {
            assert_eq!(
                cache.get(&first, CompleteBodyIndex::new).faces(1),
                Some(vec![10])
            );
            assert_eq!(
                cache.get(&second, CompleteBodyIndex::new).faces(1),
                Some(vec![11])
            );
        }
    }
}
