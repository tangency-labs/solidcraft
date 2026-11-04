// SPDX-License-Identifier: Apache-2.0
//! Index persistent recipe selectors once per immutable history snapshot.

use crate::history_records::AsmHistoricalTopology;
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use std::collections::{HashMap, HashSet};

type TokenReferences<'a> = HashMap<&'a str, HashMap<i64, Vec<usize>>>;

pub(super) struct RecipeTopologyIndex<'a> {
    pub(super) topology: &'a AsmHistoricalTopology,
    tokens: TokenReferences<'a>,
    faces: Option<HashMap<i64, Vec<usize>>>,
}

impl<'a> RecipeTopologyIndex<'a> {
    fn new(topology: &'a AsmHistoricalTopology) -> Self {
        let live_faces = topology.faces.iter().copied().collect::<HashSet<_>>();
        let live_edges = topology.edges.iter().copied().collect::<HashSet<_>>();
        let mut tokens = TokenReferences::new();
        for (index, tag) in topology.persistent_subentity_tags.iter().enumerate() {
            let live = match tag.entity_kind {
                AsmHistoricalEntityKind::Face => live_faces.contains(&tag.entity_ref),
                AsmHistoricalEntityKind::Edge => live_edges.contains(&tag.entity_ref),
                _ => false,
            };
            if !live || tag.design_references.is_empty() {
                continue;
            }
            let references = tokens.entry(tag.token.as_str()).or_default();
            for reference in &tag.design_references {
                let rows = references.entry(*reference).or_default();
                // One tag can repeat its Design reference; the old membership query
                // selected that tag once, so retain one row in each reference group.
                if rows.last() != Some(&index) {
                    rows.push(index);
                }
            }
        }
        Self {
            topology,
            tokens,
            faces: None,
        }
    }

    pub(super) fn matching_tags(&self, token: &str, reference: i64) -> &[usize] {
        self.tokens
            .get(token)
            .and_then(|references| references.get(&reference))
            .map_or(&[], Vec::as_slice)
    }

    pub(super) fn face_tags(&mut self, reference: i64) -> &[usize] {
        let (topology, tokens) = (self.topology, &self.tokens);
        self.faces
            .get_or_insert_with(|| {
                let mut faces: HashMap<_, Vec<_>> = HashMap::new();
                for references in tokens.values() {
                    for (reference, tags) in references {
                        for tag_index in tags {
                            if topology.persistent_subentity_tags[*tag_index].entity_kind
                                == AsmHistoricalEntityKind::Face
                            {
                                faces.entry(*reference).or_default().push(*tag_index);
                            }
                        }
                    }
                }
                for tags in faces.values_mut() {
                    tags.sort_unstable_by(Ord::cmp);
                }
                faces
            })
            .get(&reference)
            .map_or(&[], Vec::as_slice)
    }
}

#[derive(Default)]
pub(super) struct RecipeTopologyCache<'a> {
    entries: HashMap<*const AsmHistoricalTopology, RecipeTopologyIndex<'a>>,
}

impl<'a> RecipeTopologyCache<'a> {
    pub(super) fn get(
        &mut self,
        topology: &'a AsmHistoricalTopology,
    ) -> &mut RecipeTopologyIndex<'a> {
        self.entries
            .entry(std::ptr::from_ref(topology))
            .or_insert_with(|| RecipeTopologyIndex::new(topology))
    }
}

#[cfg(test)]
mod tests {
    use super::RecipeTopologyCache;
    use crate::history_records::{AsmHistoricalPersistentSubentityTag, AsmHistoricalTopology};
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;

    #[test]
    fn recipe_queries_reuse_indexes_and_keep_snapshots_separate() {
        let topology = |entity_ref| AsmHistoricalTopology {
            faces: vec![entity_ref],
            persistent_subentity_tags: vec![AsmHistoricalPersistentSubentityTag {
                entity_kind: AsmHistoricalEntityKind::Face,
                entity_ref,
                selector: 0,
                token: "rim".into(),
                design_references: vec![301, 301],
                ordinal: 0,
            }],
            ..AsmHistoricalTopology::default()
        };
        let mut first = topology(10);
        let mut dead = first.persistent_subentity_tags[0].clone();
        dead.entity_ref = 999;
        dead.token = "dead".into();
        first.persistent_subentity_tags.push(dead);
        let second = topology(20);
        let mut cache = RecipeTopologyCache::default();
        for _ in 0..1_000 {
            for (topology, entity) in [(&first, 10), (&second, 20)] {
                let index = cache.get(topology);
                assert_eq!(index.matching_tags("rim", 301), &[0]);
                assert_eq!(index.face_tags(301), &[0]);
                assert_eq!(
                    index.topology.persistent_subentity_tags[0].entity_ref,
                    entity
                );
                assert!(index.matching_tags("rim", 302).is_empty());
                assert!(index.matching_tags("other", 301).is_empty());
                assert!(index.matching_tags("dead", 301).is_empty());
            }
        }
    }
    #[test]
    fn token_only_recipe_queries_match_tags_without_a_face_index() {
        let topology = AsmHistoricalTopology {
            faces: vec![10],
            persistent_subentity_tags: vec![AsmHistoricalPersistentSubentityTag {
                entity_kind: AsmHistoricalEntityKind::Face,
                entity_ref: 10,
                selector: 0,
                token: "rim".into(),
                design_references: vec![301],
                ordinal: 0,
            }],
            ..Default::default()
        };
        let mut cache = RecipeTopologyCache::default();
        for _ in 0..1_000 {
            assert_eq!(cache.get(&topology).matching_tags("rim", 301), &[0]);
        }
    }
}
