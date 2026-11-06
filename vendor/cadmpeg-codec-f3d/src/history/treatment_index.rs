// SPDX-License-Identifier: Apache-2.0
//! Discover treatment supports once per immutable topology transition.
use crate::history_records::AsmHistoricalTopology;
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

type TransitionKey = (*const AsmHistoricalTopology, *const AsmHistoricalTopology);
pub(super) struct TreatmentIndex {
    pub(super) result_boundaries: HashMap<i64, HashSet<i64>>,
    pub(super) preceding_boundaries: HashMap<i64, HashSet<i64>>,
    pub(super) supports: HashMap<i64, (i64, Vec<i64>)>,
    pub(super) radii: HashMap<i64, Option<f64>>,
    pub(super) spines: HashMap<i64, super::blend_spine::BlendSpine>,
    pub(super) preceding_edges: HashMap<i64, super::blend_spine::EdgeGeometry>,
}
impl TreatmentIndex {
    fn new(result: &AsmHistoricalTopology, preceding: &AsmHistoricalTopology) -> Self {
        let result_boundaries = super::face_boundary_edge_index(result);
        let preceding_boundaries = super::face_boundary_edge_index(preceding);
        let rows =
            super::treatment_face_supports(&result.faces, result, preceding, &result_boundaries);
        let supports = rows
            .into_iter()
            .map(|(face, carrier, supports)| (face, (carrier, supports)))
            .collect::<HashMap<_, _>>();
        let mut radii = HashMap::new();
        for radius in &result.surface_radii {
            radii
                .entry(radius.surface)
                .and_modify(|value| *value = None)
                .or_insert(Some(radius.radius));
        }
        Self {
            result_boundaries,
            preceding_boundaries,
            supports,
            radii,
            spines: super::blend_spine::blend_spines(result),
            preceding_edges: super::blend_spine::edge_geometry(preceding),
        }
    }
}
#[derive(Default)]
pub(super) struct TreatmentCache<'a> {
    entries: HashMap<TransitionKey, TreatmentIndex>,
    snapshots: PhantomData<&'a AsmHistoricalTopology>,
}
impl<'a> TreatmentCache<'a> {
    pub(super) fn get(
        &mut self,
        result: &'a AsmHistoricalTopology,
        preceding: &'a AsmHistoricalTopology,
    ) -> &TreatmentIndex {
        self.entries
            .entry((std::ptr::from_ref(result), std::ptr::from_ref(preceding)))
            .or_insert_with(|| TreatmentIndex::new(result, preceding))
    }
}
