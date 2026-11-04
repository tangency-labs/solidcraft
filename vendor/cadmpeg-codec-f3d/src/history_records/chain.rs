// SPDX-License-Identifier: Apache-2.0
//! Sorted historical row diffs and lazy chain materialization.
use super::{
    AsmDeltaState, AsmEntityVersion, AsmHistoricalEntityDelta, AsmHistoricalEntityTags,
    AsmHistoricalPersistentSubentityTag, AsmHistoricalRow, AsmHistoricalRowDelta,
    AsmHistoricalRowsDelta, AsmHistoricalSlotRepeat, AsmHistoricalTopology,
    AsmHistoricalTopologyDelta, AsmHistoricalTransition, AsmHistory, AsmTopologyScope,
    AsmTransitionError,
};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

mod membership;
use membership::Memberships;

/// One complete state as the diff reads it.
#[derive(Clone, Copy)]
pub(crate) struct AsmStateSnapshot<'a> {
    pub(crate) state_id: i64,
    pub(crate) topology: &'a AsmHistoricalTopology,
    pub(crate) versions: &'a BTreeMap<i64, i64>,
}

#[derive(Default)]
pub(crate) struct AsmIdentityMemberships {
    pub(crate) revisions: Vec<(usize, AsmEntityVersion)>,
    pub(crate) entities: Vec<(usize, i64, AsmHistoricalEntityKind)>,
    pub(crate) all_states_available: bool,
}

/// Derived state topologies. Clones start without materializations.
#[derive(Default)]
pub(crate) struct MaterializedTopologies {
    states: OnceLock<Vec<OnceLock<Option<Box<AsmHistoricalTopology>>>>>,
    order: OnceLock<Result<Option<Vec<usize>>, AsmTransitionError>>,
    transition_validity: OnceLock<Vec<bool>>,
}

impl Clone for MaterializedTopologies {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl PartialEq for MaterializedTopologies {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl std::fmt::Debug for MaterializedTopologies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MaterializedTopologies")
    }
}

fn strictly_sorted(keys: impl IntoIterator<Item = i64>) -> bool {
    let mut previous = None;
    for key in keys {
        if previous.is_some_and(|previous| previous >= key) {
            return false;
        }
        previous = Some(key);
    }
    true
}

fn entity_delta(
    newer: &[i64],
    older: &[i64],
    new: &BTreeMap<i64, i64>,
    old: &BTreeMap<i64, i64>,
) -> Option<AsmHistoricalEntityDelta> {
    if !strictly_sorted(newer.iter().copied()) || !strictly_sorted(older.iter().copied()) {
        return None;
    }
    let mut result = AsmHistoricalEntityDelta::default();
    let mut new_keys = newer.iter().peekable();
    let mut old_keys = older.iter().peekable();
    while let (Some(&newer), Some(&older)) = (new_keys.peek(), old_keys.peek()) {
        match newer.cmp(older) {
            std::cmp::Ordering::Less => {
                result.inserted.push(*newer);
                new_keys.next();
            }
            std::cmp::Ordering::Greater => {
                result.deleted.push(*older);
                old_keys.next();
            }
            std::cmp::Ordering::Equal => {
                if new.get(newer) != old.get(older) {
                    result.updated.push(*newer);
                }
                new_keys.next();
                old_keys.next();
            }
        }
    }
    result.inserted.extend(new_keys);
    result.deleted.extend(old_keys);
    Some(result)
}

fn ordered_keys<R: AsmHistoricalRow>(rows: &[R]) -> bool {
    if R::SHARED_KEYS {
        rows.windows(2).all(|pair| pair[0].key() <= pair[1].key())
    } else {
        strictly_sorted(rows.iter().map(R::key))
    }
}

/// Split the leading rows whose key is `key` from the rest.
fn split_key<R: AsmHistoricalRow>(rows: &[R], key: i64) -> (&[R], &[R]) {
    rows.split_at(rows.iter().take_while(|row| row.key() == key).count())
}

impl<R: AsmHistoricalRow> AsmHistoricalRowDelta<R> {
    /// A changed key sets all of its rows.
    fn between(older: &[R], newer: &[R]) -> Option<Self> {
        if !ordered_keys(older) || !ordered_keys(newer) {
            return None;
        }
        let mut result = Self::default();
        let (mut old, mut new) = (older, newer);
        while let (Some(older), Some(newer)) = (old.first(), new.first()) {
            let key = older.key().min(newer.key());
            let (old_rows, old_rest) = split_key(old, key);
            let (new_rows, new_rest) = split_key(new, key);
            if new_rows.is_empty() {
                result.removed.push(key);
            } else if old_rows != new_rows {
                result.set.extend_from_slice(new_rows);
            }
            (old, new) = (old_rest, new_rest);
        }
        while let Some(older) = old.first() {
            result.removed.push(older.key());
            old = split_key(old, older.key()).1;
        }
        result.set.extend_from_slice(new);
        Some(result)
    }

    fn apply(&self, family: &mut Vec<R>) -> Result<(), AsmTransitionError> {
        if self.is_empty() {
            return Ok(());
        }
        if !ordered_keys(&self.set)
            || !strictly_sorted(self.removed.iter().copied())
            || !ordered_keys(family)
        {
            return Err(AsmTransitionError::UnsortedFamily);
        }
        replace_key_runs(
            family,
            R::key,
            self.set
                .chunk_by(|left, right| left.key() == right.key())
                .map(|rows| (rows[0].key(), rows)),
            &self.removed,
        )
    }
}

/// Replace the run of rows with each set key by that key's rows and drop the
/// run of each removed key, in a family sorted by key. Rows before the first
/// run whose length changes stay in place.
fn replace_key_runs<'a, R: Clone + 'a>(
    family: &mut Vec<R>,
    key: impl Fn(&R) -> i64,
    set: impl IntoIterator<Item = (i64, &'a [R])>,
    removed: &[i64],
) -> Result<(), AsmTransitionError> {
    let run = |family: &[R], target: i64| {
        let start = family.partition_point(|row| key(row) < target);
        start..start + family[start..].partition_point(|row| key(row) <= target)
    };
    let mut set = set.into_iter().peekable();
    let mut removed = removed.iter().copied().peekable();
    let mut resized = Vec::new();
    loop {
        let next_removed = removed.peek().copied();
        let (target, rows, remove) = match set.peek() {
            Some(&(target, rows)) if next_removed.is_none_or(|removed| target <= removed) => {
                set.next();
                (target, rows, next_removed == Some(target))
            }
            _ => match next_removed {
                Some(target) => (target, &[][..], true),
                None => break,
            },
        };
        if remove {
            removed.next();
        }
        let run = run(family, target);
        if remove && run.is_empty() {
            return Err(AsmTransitionError::RemovedRowAbsent);
        }
        if run.len() == rows.len() {
            family[run].clone_from_slice(rows);
        } else {
            resized.push((run, rows));
        }
    }
    let Some(first) = resized.first().map(|(run, _)| run.start) else {
        return Ok(());
    };
    let (kept, replacing) = resized
        .iter()
        .fold((0, 0), |(kept, replacing), (run, rows)| {
            (kept + run.len(), replacing + rows.len())
        });
    if let Some(growth) = replacing.checked_sub(kept) {
        family
            .try_reserve_exact(growth)
            .map_err(|_| AsmTransitionError::Allocation)?;
    }
    let mut tail = family.split_off(first).into_iter();
    let mut position = first;
    for (run, rows) in resized {
        family.extend(tail.by_ref().take(run.start - position));
        family.extend_from_slice(rows);
        tail.by_ref().take(run.len()).for_each(drop);
        position = run.end;
    }
    family.extend(tail);
    Ok(())
}

fn entity_tags(topology: &AsmHistoricalTopology) -> Vec<AsmHistoricalEntityTags> {
    group_entity_tags(topology.persistent_subentity_tags.iter().cloned())
}

fn group_entity_tags(
    tags: impl IntoIterator<Item = AsmHistoricalPersistentSubentityTag>,
) -> Vec<AsmHistoricalEntityTags> {
    let mut groups: Vec<AsmHistoricalEntityTags> = Vec::new();
    for tag in tags {
        if let Some(last) = groups
            .last_mut()
            .filter(|last| last.entity_ref == tag.entity_ref)
        {
            last.tags.push(tag);
        } else {
            groups.push(AsmHistoricalEntityTags {
                entity_ref: tag.entity_ref,
                tags: vec![tag],
            });
        }
    }
    groups
}

fn slot_repeats(family: &[i64]) -> Option<Vec<AsmHistoricalSlotRepeat>> {
    let mut groups: Vec<AsmHistoricalSlotRepeat> = Vec::new();
    let mut previous = None;
    let mut count = 0_u32;
    for &slot in family {
        if previous == Some(slot) {
            count = count.checked_add(1)?;
        } else {
            if previous.is_some_and(|previous| previous > slot) {
                return None;
            }
            if let Some(slot) = previous.filter(|_| count >= 2) {
                groups.push(AsmHistoricalSlotRepeat { slot, count });
            }
            previous = Some(slot);
            count = 1;
        }
    }
    if let Some(slot) = previous.filter(|_| count >= 2) {
        groups.push(AsmHistoricalSlotRepeat { slot, count });
    }
    Some(groups)
}

fn distinct_slots(family: &[i64]) -> Vec<i64> {
    let mut slots = family.to_vec();
    slots.dedup();
    slots
}

fn apply_slot_repeats(
    membership: &AsmHistoricalEntityDelta,
    rows: &AsmHistoricalRowDelta<AsmHistoricalSlotRepeat>,
    family: &mut Vec<i64>,
    updated_records: &[i64],
) -> Result<(), AsmTransitionError> {
    if rows.is_empty()
        && membership.inserted.is_empty()
        && membership.deleted.is_empty()
        && membership.updated.is_empty()
    {
        return Ok(());
    }
    let mut repeats = slot_repeats(family).ok_or(AsmTransitionError::UnsortedFamily)?;
    let mut slots = distinct_slots(family);
    apply_membership(membership, &mut slots, updated_records)?;
    rows.apply(&mut repeats)?;
    if repeats
        .iter()
        .any(|repeat| repeat.count < 2 || slots.binary_search(&repeat.slot).is_err())
    {
        return Err(AsmTransitionError::MembershipCoverage);
    }
    let mut repeats = repeats.iter().peekable();
    family.clear();
    for slot in slots {
        let count = if repeats.peek().is_some_and(|repeat| repeat.slot == slot) {
            repeats.next().map_or(1, |repeat| repeat.count)
        } else {
            1
        };
        let count = usize::try_from(count).map_err(|_| AsmTransitionError::MembershipCoverage)?;
        family
            .try_reserve(count)
            .map_err(|_| AsmTransitionError::MembershipCoverage)?;
        for _ in 0..count {
            family.push(slot);
        }
    }
    Ok(())
}

impl AsmHistoricalTransition {
    /// Forward change from `older`, or empty before the tail, to `newer`.
    pub(crate) fn between(
        older: Option<AsmStateSnapshot<'_>>,
        newer: AsmStateSnapshot<'_>,
    ) -> Option<Self> {
        let empty = AsmHistoricalTopology::default();
        let empty_versions = BTreeMap::new();
        let old_topology = older.as_ref().map_or(&empty, |state| state.topology);
        let old_versions = older
            .as_ref()
            .map_or(&empty_versions, |state| state.versions);
        let new = newer.topology;

        let old_surfaces = slot_repeats(&old_topology.surfaces)?;
        let new_surfaces = slot_repeats(&new.surfaces)?;
        let old_curves = slot_repeats(&old_topology.curves)?;
        let new_curves = slot_repeats(&new.curves)?;
        let delta = |new: &[i64], old: &[i64]| entity_delta(new, old, newer.versions, old_versions);
        let records = delta(
            &newer.versions.keys().copied().collect::<Vec<_>>(),
            &old_versions.keys().copied().collect::<Vec<_>>(),
        )?;
        let revisions: BTreeSet<_> = records
            .inserted
            .iter()
            .chain(&records.updated)
            .copied()
            .collect();
        let record_revisions = revisions
            .into_iter()
            .map(|entity_ref| {
                Some(AsmEntityVersion {
                    entity_ref,
                    record_ref: *newer.versions.get(&entity_ref)?,
                })
            })
            .collect::<Option<_>>()?;
        Some(Self {
            previous_state_id: older.map(|state| state.state_id),
            records,
            record_revisions,
            topology: AsmHistoricalTopologyDelta {
                bodies: delta(&new.bodies, &old_topology.bodies)?,
                regions: delta(&new.regions, &old_topology.regions)?,
                shells: delta(&new.shells, &old_topology.shells)?,
                faces: delta(&new.faces, &old_topology.faces)?,
                loops: delta(&new.loops, &old_topology.loops)?,
                coedges: delta(&new.coedges, &old_topology.coedges)?,
                edges: delta(&new.edges, &old_topology.edges)?,
                vertices: delta(&new.vertices, &old_topology.vertices)?,
                points: delta(&new.points, &old_topology.points)?,
                surfaces: delta(
                    &distinct_slots(&new.surfaces),
                    &distinct_slots(&old_topology.surfaces),
                )?,
                curves: delta(
                    &distinct_slots(&new.curves),
                    &distinct_slots(&old_topology.curves),
                )?,
                pcurves: delta(&new.pcurves, &old_topology.pcurves)?,
            },
            rows: AsmHistoricalRowsDelta {
                surface_repeats: AsmHistoricalRowDelta::between(&old_surfaces, &new_surfaces)?,
                curve_repeats: AsmHistoricalRowDelta::between(&old_curves, &new_curves)?,
                surface_radii: AsmHistoricalRowDelta::between(
                    &old_topology.surface_radii,
                    &new.surface_radii,
                )?,
                surface_cylinders: AsmHistoricalRowDelta::between(
                    &old_topology.surface_cylinders,
                    &new.surface_cylinders,
                )?,
                surface_planes: AsmHistoricalRowDelta::between(
                    &old_topology.surface_planes,
                    &new.surface_planes,
                )?,
                surface_axes: AsmHistoricalRowDelta::between(
                    &old_topology.surface_axes,
                    &new.surface_axes,
                )?,
                curve_axes: AsmHistoricalRowDelta::between(
                    &old_topology.curve_axes,
                    &new.curve_axes,
                )?,
                persistent_subentity_tags: AsmHistoricalRowDelta::between(
                    &entity_tags(old_topology),
                    &entity_tags(new),
                )?,
                body_regions: AsmHistoricalRowDelta::between(
                    &old_topology.body_regions,
                    &new.body_regions,
                )?,
                region_shells: AsmHistoricalRowDelta::between(
                    &old_topology.region_shells,
                    &new.region_shells,
                )?,
                shell_faces: AsmHistoricalRowDelta::between(
                    &old_topology.shell_faces,
                    &new.shell_faces,
                )?,
                shell_wire_edges: AsmHistoricalRowDelta::between(
                    &old_topology.shell_wire_edges,
                    &new.shell_wire_edges,
                )?,
                shell_free_vertices: AsmHistoricalRowDelta::between(
                    &old_topology.shell_free_vertices,
                    &new.shell_free_vertices,
                )?,
                face_loops: AsmHistoricalRowDelta::between(
                    &old_topology.face_loops,
                    &new.face_loops,
                )?,
                loop_coedges: AsmHistoricalRowDelta::between(
                    &old_topology.loop_coedges,
                    &new.loop_coedges,
                )?,
                coedge_topology: AsmHistoricalRowDelta::between(
                    &old_topology.coedge_topology,
                    &new.coedge_topology,
                )?,
                edge_vertices: AsmHistoricalRowDelta::between(
                    &old_topology.edge_vertices,
                    &new.edge_vertices,
                )?,
                face_surfaces: AsmHistoricalRowDelta::between(
                    &old_topology.face_surfaces,
                    &new.face_surfaces,
                )?,
                vertex_points: AsmHistoricalRowDelta::between(
                    &old_topology.vertex_points,
                    &new.vertex_points,
                )?,
                edge_curves: AsmHistoricalRowDelta::between(
                    &old_topology.edge_curves,
                    &new.edge_curves,
                )?,
                coedge_pcurves: AsmHistoricalRowDelta::between(
                    &old_topology.coedge_pcurves,
                    &new.coedge_pcurves,
                )?,
                point_positions: AsmHistoricalRowDelta::between(
                    &old_topology.point_positions,
                    &new.point_positions,
                )?,
                edge_samples: AsmHistoricalRowDelta::between(
                    &old_topology.edge_samples,
                    &new.edge_samples,
                )?,
            },
        })
    }

    fn apply_topology(
        &self,
        topology: &mut AsmHistoricalTopology,
    ) -> Result<(), AsmTransitionError> {
        apply_membership(
            &self.topology.bodies,
            &mut topology.bodies,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.regions,
            &mut topology.regions,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.shells,
            &mut topology.shells,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.faces,
            &mut topology.faces,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.loops,
            &mut topology.loops,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.coedges,
            &mut topology.coedges,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.edges,
            &mut topology.edges,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.vertices,
            &mut topology.vertices,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.points,
            &mut topology.points,
            &self.records.updated,
        )?;
        apply_slot_repeats(
            &self.topology.surfaces,
            &self.rows.surface_repeats,
            &mut topology.surfaces,
            &self.records.updated,
        )?;
        apply_slot_repeats(
            &self.topology.curves,
            &self.rows.curve_repeats,
            &mut topology.curves,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.pcurves,
            &mut topology.pcurves,
            &self.records.updated,
        )?;
        self.rows.surface_radii.apply(&mut topology.surface_radii)?;
        self.rows
            .surface_cylinders
            .apply(&mut topology.surface_cylinders)?;
        self.rows
            .surface_planes
            .apply(&mut topology.surface_planes)?;
        self.rows.surface_axes.apply(&mut topology.surface_axes)?;
        self.rows.curve_axes.apply(&mut topology.curve_axes)?;
        self.rows.body_regions.apply(&mut topology.body_regions)?;
        self.rows.region_shells.apply(&mut topology.region_shells)?;
        self.rows.shell_faces.apply(&mut topology.shell_faces)?;
        self.rows
            .shell_wire_edges
            .apply(&mut topology.shell_wire_edges)?;
        self.rows
            .shell_free_vertices
            .apply(&mut topology.shell_free_vertices)?;
        self.rows.face_loops.apply(&mut topology.face_loops)?;
        self.rows.loop_coedges.apply(&mut topology.loop_coedges)?;
        self.rows
            .coedge_topology
            .apply(&mut topology.coedge_topology)?;
        self.rows.edge_vertices.apply(&mut topology.edge_vertices)?;
        self.rows.face_surfaces.apply(&mut topology.face_surfaces)?;
        self.rows.vertex_points.apply(&mut topology.vertex_points)?;
        self.rows.edge_curves.apply(&mut topology.edge_curves)?;
        self.rows
            .coedge_pcurves
            .apply(&mut topology.coedge_pcurves)?;
        self.rows
            .point_positions
            .apply(&mut topology.point_positions)?;
        self.rows.edge_samples.apply(&mut topology.edge_samples)?;
        let tags = &self.rows.persistent_subentity_tags;
        if !tags.is_empty() {
            if tags.set.iter().any(|group| {
                group.tags.is_empty()
                    || group
                        .tags
                        .iter()
                        .any(|tag| tag.entity_ref != group.entity_ref)
            }) || !strictly_sorted(tags.set.iter().map(|group| group.entity_ref))
                || !strictly_sorted(tags.removed.iter().copied())
                || !topology
                    .persistent_subentity_tags
                    .is_sorted_by_key(|tag| tag.entity_ref)
            {
                return Err(AsmTransitionError::UnsortedFamily);
            }
            replace_key_runs(
                &mut topology.persistent_subentity_tags,
                |tag| tag.entity_ref,
                tags.set
                    .iter()
                    .map(|group| (group.entity_ref, group.tags.as_slice())),
                &tags.removed,
            )?;
        }
        Ok(())
    }

    fn check_revision_coverage(&self) -> Result<(), AsmTransitionError> {
        check_delta_order(&self.records)?;
        if !strictly_sorted(self.record_revisions.iter().map(|row| row.entity_ref)) {
            return Err(AsmTransitionError::UnsortedFamily);
        }
        let mut inserted = self.records.inserted.iter().peekable();
        let mut updated = self.records.updated.iter().peekable();
        for row in &self.record_revisions {
            match (inserted.peek().copied(), updated.peek().copied()) {
                (Some(a), Some(b)) if a == b => return Err(AsmTransitionError::RevisionCoverage),
                (Some(a), Some(b)) if a < b && *a == row.entity_ref => {
                    inserted.next();
                }
                (Some(_), Some(b)) if *b == row.entity_ref => {
                    updated.next();
                }
                (Some(a), None) if *a == row.entity_ref => {
                    inserted.next();
                }
                (None, Some(b)) if *b == row.entity_ref => {
                    updated.next();
                }
                _ => return Err(AsmTransitionError::RevisionCoverage),
            }
        }
        if inserted.next().is_some()
            || updated.next().is_some()
            || intersects(&self.records.deleted, &self.records.inserted)
            || intersects(&self.records.deleted, &self.records.updated)
        {
            return Err(AsmTransitionError::RevisionCoverage);
        }
        Ok(())
    }

    fn apply_version_map(
        &self,
        versions: &mut BTreeMap<i64, i64>,
    ) -> Result<(), AsmTransitionError> {
        self.check_revision_coverage()?;
        for revision in &self.record_revisions {
            if self
                .records
                .inserted
                .binary_search(&revision.entity_ref)
                .is_ok()
            {
                if versions.contains_key(&revision.entity_ref) {
                    return Err(AsmTransitionError::InsertedPresent);
                }
            } else if versions
                .get(&revision.entity_ref)
                .is_none_or(|old| *old == revision.record_ref)
            {
                return Err(AsmTransitionError::InvalidUpdate);
            }
        }
        for slot in &self.records.deleted {
            if versions.remove(slot).is_none() {
                return Err(AsmTransitionError::DeletedAbsent);
            }
        }
        versions.extend(
            self.record_revisions
                .iter()
                .map(|row| (row.entity_ref, row.record_ref)),
        );
        Ok(())
    }

    fn apply_versions(
        &self,
        versions: &mut Vec<AsmEntityVersion>,
    ) -> Result<(), AsmTransitionError> {
        self.check_revision_coverage()?;
        let mut old = versions.iter().peekable();
        let mut inserted = self.records.inserted.iter().peekable();
        for revision in &self.record_revisions {
            while old
                .peek()
                .is_some_and(|row| row.entity_ref < revision.entity_ref)
            {
                old.next();
            }
            let previous = old
                .peek()
                .filter(|row| row.entity_ref == revision.entity_ref);
            if inserted
                .peek()
                .is_some_and(|&&key| key == revision.entity_ref)
            {
                if previous.is_some() {
                    return Err(AsmTransitionError::InsertedPresent);
                }
                inserted.next();
            } else if previous.is_none_or(|row| row.record_ref == revision.record_ref) {
                return Err(AsmTransitionError::InvalidUpdate);
            }
        }
        let delta = AsmHistoricalRowDelta {
            set: self.record_revisions.clone(),
            removed: self.records.deleted.clone(),
        };
        delta.apply(versions).map_err(|error| match error {
            AsmTransitionError::RemovedRowAbsent => AsmTransitionError::DeletedAbsent,
            error => error,
        })
    }
}

impl AsmHistoricalRow for AsmEntityVersion {
    fn key(&self) -> i64 {
        self.entity_ref
    }
}

fn intersects(left: &[i64], right: &[i64]) -> bool {
    let mut right = right.iter().peekable();
    left.iter().any(|key| {
        while right.peek().is_some_and(|&other| other < key) {
            right.next();
        }
        right.peek().is_some_and(|&other| other == key)
    })
}

fn check_delta_order(delta: &AsmHistoricalEntityDelta) -> Result<(), AsmTransitionError> {
    if [&delta.inserted, &delta.deleted, &delta.updated]
        .iter()
        .any(|keys| !strictly_sorted(keys.iter().copied()))
    {
        return Err(AsmTransitionError::UnsortedFamily);
    }
    Ok(())
}

fn apply_membership(
    delta: &AsmHistoricalEntityDelta,
    family: &mut Vec<i64>,
    updated_records: &[i64],
) -> Result<(), AsmTransitionError> {
    if delta.inserted.is_empty() && delta.deleted.is_empty() && delta.updated.is_empty() {
        return Ok(());
    }
    check_delta_order(delta)?;
    if !strictly_sorted(family.iter().copied()) {
        return Err(AsmTransitionError::UnsortedFamily);
    }
    let mut revisions = updated_records.iter().peekable();
    let mut deleted = delta.deleted.iter().peekable();
    let expected_updates = family.iter().copied().filter(|slot| {
        while revisions.peek().is_some_and(|&&key| key < *slot) {
            revisions.next();
        }
        while deleted.peek().is_some_and(|&&key| key < *slot) {
            deleted.next();
        }
        revisions.peek().is_some_and(|&&key| key == *slot)
            && deleted.peek().is_none_or(|&&key| key != *slot)
    });
    if !expected_updates.eq(delta.updated.iter().copied()) {
        return Err(AsmTransitionError::InvalidUpdate);
    }
    if intersects(&delta.inserted, &delta.deleted) {
        return Err(AsmTransitionError::InsertedPresent);
    }
    let present = |slot: &&i64| family.binary_search(slot).is_ok();
    // A key-order merge of the family and the insertions meets an inserted
    // slot already present at that slot, and an absent deletion only at the
    // next merged slot above it.
    let follows = |keys: &[i64], low: i64, high: i64| {
        keys.get(keys.partition_point(|&key| key <= low))
            .is_some_and(|&key| key < high)
    };
    match (
        delta.inserted.iter().find(present),
        delta.deleted.iter().find(|slot| !present(slot)),
    ) {
        (Some(&inserted), Some(&absent))
            if absent < inserted
                && (follows(family, absent, inserted)
                    || follows(&delta.inserted, absent, inserted)) =>
        {
            return Err(AsmTransitionError::DeletedAbsent);
        }
        (Some(_), _) => return Err(AsmTransitionError::InsertedPresent),
        (None, Some(_)) => return Err(AsmTransitionError::DeletedAbsent),
        (None, None) => {}
    }
    replace_key_runs(
        family,
        |slot| *slot,
        delta
            .inserted
            .iter()
            .map(|slot| (*slot, std::slice::from_ref(slot))),
        &delta.deleted,
    )
}

impl AsmHistoricalRowsDelta {
    pub(crate) fn retain_projection_rows(&mut self) {
        self.surface_radii = AsmHistoricalRowDelta::default();
        self.surface_axes = AsmHistoricalRowDelta::default();
        self.persistent_subentity_tags = AsmHistoricalRowDelta::default();
        self.body_regions = AsmHistoricalRowDelta::default();
        self.region_shells = AsmHistoricalRowDelta::default();
        self.shell_faces = AsmHistoricalRowDelta::default();
        self.shell_wire_edges = AsmHistoricalRowDelta::default();
        self.shell_free_vertices = AsmHistoricalRowDelta::default();
        self.edge_vertices = AsmHistoricalRowDelta::default();
        self.vertex_points = AsmHistoricalRowDelta::default();
        self.point_positions = AsmHistoricalRowDelta::default();
        self.edge_samples = AsmHistoricalRowDelta::default();
    }
}

impl AsmHistoricalTopology {
    pub(crate) fn has_retained_rows(&self) -> bool {
        !self.face_surfaces.is_empty()
            || !self.surface_planes.is_empty()
            || !self.surface_cylinders.is_empty()
            || !self.loop_coedges.is_empty()
            || !self.coedge_topology.is_empty()
            || !self.face_loops.is_empty()
            || !self.edge_curves.is_empty()
            || !self.coedge_pcurves.is_empty()
            || !self.curve_axes.is_empty()
    }
    pub(crate) fn slots(&self) -> BTreeSet<i64> {
        [
            &self.bodies,
            &self.regions,
            &self.shells,
            &self.faces,
            &self.loops,
            &self.coedges,
            &self.edges,
            &self.vertices,
            &self.points,
            &self.surfaces,
            &self.curves,
            &self.pcurves,
        ]
        .into_iter()
        .flatten()
        .copied()
        .collect()
    }
}

impl AsmHistory {
    fn chain_order(&self) -> Result<Vec<usize>, AsmTransitionError> {
        let by_node: BTreeMap<_, _> = self
            .states
            .iter()
            .enumerate()
            .map(|(i, state)| (state.node_index, i))
            .collect();
        if by_node.len() != self.states.len() {
            return Err(AsmTransitionError::Chain);
        }
        let mut heads = self
            .states
            .iter()
            .enumerate()
            .filter(|(_, state)| state.previous_ref.is_none());
        let Some((head, _)) = heads.next() else {
            return Err(AsmTransitionError::Chain);
        };
        if heads.next().is_some() {
            return Err(AsmTransitionError::Chain);
        }
        let mut order = Vec::new();
        let mut seen = BTreeSet::new();
        let mut next = Some(head);
        while let Some(index) = next {
            if !seen.insert(index) {
                return Err(AsmTransitionError::Chain);
            }
            order.push(index);
            next = match self.states[index].next_ref {
                Some(node) => Some(*by_node.get(&node).ok_or(AsmTransitionError::Chain)?),
                None => None,
            };
        }
        if order.len() != self.states.len() {
            return Err(AsmTransitionError::Chain);
        }
        Ok(order)
    }

    fn bound_chain(&self) -> Result<Option<Vec<usize>>, AsmTransitionError> {
        let count = self
            .states
            .iter()
            .filter(|state| state.transition.is_some())
            .count();
        if count == 0 {
            return Ok(None);
        }
        if count != self.states.len() {
            return Err(AsmTransitionError::MissingTransition);
        }
        let order = self.chain_order()?;
        for (position, &index) in order.iter().enumerate() {
            let expected = order
                .get(position + 1)
                .map(|&older| self.states[older].state_id);
            let transition = self.states[index]
                .transition
                .as_ref()
                .ok_or(AsmTransitionError::MissingTransition)?;
            if transition.previous_state_id != expected {
                return Err(AsmTransitionError::PreviousState);
            }
        }
        Ok(Some(order))
    }

    /// Check chain links and all delta application invariants.
    pub(crate) fn check_transitions(&self) -> Result<(), AsmTransitionError> {
        let Some(order) = self.bound_chain()? else {
            return Ok(());
        };
        let mut topology = AsmHistoricalTopology::default();
        let mut versions = Vec::new();
        for index in order.into_iter().rev() {
            let transition = self.states[index]
                .transition
                .as_ref()
                .ok_or(AsmTransitionError::MissingTransition)?;
            transition.apply_versions(&mut versions)?;
            transition.apply_topology(&mut topology)?;
        }
        Ok(())
    }

    fn walk_memberships(
        &self,
        order: &[usize],
        mut visit: impl FnMut(usize, &Memberships),
    ) -> Vec<bool> {
        let mut validity = vec![false; self.states.len()];
        let mut versions = BTreeMap::new();
        let mut membership = Memberships::default();
        for &index in order.iter().rev() {
            let Some(transition) = self.states[index].transition.as_ref() else {
                break;
            };
            if transition.apply_version_map(&mut versions).is_err()
                || membership.apply(transition).is_err()
            {
                break;
            }
            validity[index] = true;
            visit(index, &membership);
        }
        validity
    }

    fn transition_validity(&self, order: &[usize]) -> &[bool] {
        self.materialized
            .transition_validity
            .get_or_init(|| self.walk_memberships(order, |_, _| {}))
    }

    /// State indexes of the bound chain, oldest first.
    pub(crate) fn states_oldest_first(&self) -> Option<Vec<usize>> {
        let order = self
            .materialized
            .order
            .get_or_init(|| self.bound_chain())
            .as_ref()
            .ok()?
            .as_ref()?;
        Some(order.iter().rev().copied().collect())
    }

    /// Drop cached materializations except the state at `index`.
    pub(crate) fn retain_topology(&mut self, index: usize) {
        let Some(cache) = self.materialized.states.get_mut() else {
            return;
        };
        for (ordinal, state) in cache.iter_mut().enumerate() {
            if ordinal != index {
                *state = OnceLock::new();
            }
        }
    }

    /// Materialize the member state at `index` by applying its transitions to
    /// the nearest older materialization in place; that older state is no
    /// longer cached.
    pub(crate) fn advance_topology(&mut self, index: usize) {
        let Ok(Some(order)) = self.materialized.order.get_or_init(|| self.bound_chain()) else {
            return;
        };
        if self.transition_validity(order).get(index) != Some(&true) {
            return;
        }
        let Some(position) = order.iter().position(|&ordinal| ordinal == index) else {
            return;
        };
        self.materialized
            .states
            .get_or_init(|| self.states.iter().map(|_| OnceLock::new()).collect());
        let Some(cache) = self.materialized.states.get_mut() else {
            return;
        };
        if cache.get(index).is_none_or(|state| state.get().is_some()) {
            return;
        }
        let Some(older) = order
            .iter()
            .skip(position + 1)
            .position(|&older| matches!(cache.get(older).and_then(OnceLock::get), Some(Some(_))))
            .map(|offset| position + 1 + offset)
        else {
            return;
        };
        let Some(Some(mut materialized)) = cache.get_mut(order[older]).and_then(OnceLock::take)
        else {
            return;
        };
        let advanced = order[position..older]
            .iter()
            .rev()
            .try_for_each(|&ordinal| {
                let transition = self.states.get(ordinal)?.transition.as_ref()?;
                transition.apply_topology(&mut materialized).ok()
            });
        if let Some(state) = cache.get_mut(index) {
            *state = OnceLock::from(advanced.map(|()| materialized));
        }
    }

    /// Rebuild this member state's topology and cache only that state.
    pub(crate) fn topology(&self, state: &AsmDeltaState) -> Option<&AsmHistoricalTopology> {
        let index = self
            .states
            .iter()
            .position(|member| std::ptr::eq(member, state))?;
        let order = self
            .materialized
            .order
            .get_or_init(|| self.bound_chain())
            .as_ref()
            .ok()?
            .as_ref()?;
        if !self.transition_validity(order).get(index).copied()? {
            return None;
        }
        let position = order.iter().position(|&ordinal| ordinal == index)?;
        let cache = self
            .materialized
            .states
            .get_or_init(|| self.states.iter().map(|_| OnceLock::new()).collect());
        cache
            .get(index)?
            .get_or_init(|| {
                let mut materialized = AsmHistoricalTopology::default();
                let mut older_position = order.len();
                for (offset, &older) in order.iter().enumerate().skip(position + 1) {
                    if let Some(Some(cached)) = cache.get(older)?.get() {
                        materialized = cached.as_ref().clone();
                        older_position = offset;
                        break;
                    }
                }
                for &ordinal in order.get(position..older_position)?.iter().rev() {
                    let transition = self.states.get(ordinal)?.transition.as_ref()?;
                    transition.apply_topology(&mut materialized).ok()?;
                }
                Some(Box::new(materialized))
            })
            .as_deref()
            .filter(|state| {
                self.topology_scope != AsmTopologyScope::Retained || state.has_retained_rows()
            })
    }

    /// Requested revisions and entity kinds, in member-state order.
    pub(crate) fn identity_memberships(
        &self,
        requested: &std::collections::HashSet<i64>,
        entities: &std::collections::HashSet<i64>,
    ) -> AsmIdentityMemberships {
        let Some(order) = self
            .materialized
            .order
            .get_or_init(|| self.bound_chain())
            .as_ref()
            .ok()
            .and_then(Option::as_ref)
        else {
            return AsmIdentityMemberships::default();
        };
        let mut selected = BTreeMap::new();
        let mut result = AsmIdentityMemberships::default();
        let mut available = 0;
        let validity = self.walk_memberships(order, |index, membership| {
            if let Some(transition) = self.states[index].transition.as_ref() {
                for slot in &transition.records.deleted {
                    selected.remove(slot);
                }
                for version in &transition.record_revisions {
                    if requested.contains(&version.record_ref) {
                        selected.insert(version.entity_ref, version.record_ref);
                    } else {
                        selected.remove(&version.entity_ref);
                    }
                }
            }
            if self.topology_scope == AsmTopologyScope::Retained && !membership.has_retained_rows()
            {
                return;
            }
            available += 1;
            for (kind, family) in membership.families() {
                result.entities.extend(
                    entities
                        .iter()
                        .filter(|slot| family.contains(slot))
                        .map(|&slot| (index, slot, kind)),
                );
            }
            result.revisions.extend(
                selected
                    .iter()
                    .filter(|(slot, _)| membership.contains(**slot))
                    .map(|(&entity_ref, &record_ref)| {
                        (
                            index,
                            AsmEntityVersion {
                                entity_ref,
                                record_ref,
                            },
                        )
                    }),
            );
        });
        self.materialized
            .transition_validity
            .get_or_init(|| validity);
        result
            .revisions
            .sort_by_key(|(index, version)| (*index, version.entity_ref));
        result
            .entities
            .sort_by_key(|(index, slot, _)| (*index, *slot));
        result.all_states_available = available == self.states.len();
        result
    }

    /// Revisions of topology slots for independent reconstruction tests.
    #[cfg(test)]
    pub(crate) fn entity_versions(&self, state: &AsmDeltaState) -> Vec<AsmEntityVersion> {
        let Some(topology) = self.topology(state) else {
            return Vec::new();
        };
        let Ok(Some(order)) = self.bound_chain() else {
            return Vec::new();
        };
        let mut versions = Vec::new();
        for index in order.into_iter().rev() {
            let Some(transition) = self.states[index].transition.as_ref() else {
                return Vec::new();
            };
            if transition.apply_versions(&mut versions).is_err() {
                return Vec::new();
            }
            if std::ptr::eq(std::ptr::from_ref(&self.states[index]), state) {
                let slots = topology.slots();
                versions.retain(|version| slots.contains(&version.entity_ref));
                return versions;
            }
        }
        Vec::new()
    }

    /// Number of member states whose topology is currently materialized.
    #[cfg(test)]
    pub(crate) fn materialized_state_count(&self) -> usize {
        self.materialized.states.get().map_or(0, |cache| {
            cache.iter().filter(|entry| entry.get().is_some()).count()
        })
    }

    /// Drop all cached materializations.
    pub(crate) fn release_topologies(&mut self) {
        self.materialized = MaterializedTopologies::default();
    }
}
#[cfg(test)]
mod tests;
