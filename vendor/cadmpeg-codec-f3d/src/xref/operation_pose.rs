// SPDX-License-Identifier: Apache-2.0
//! Definition-local poses from direct operation-table paths (spec §1.4).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent};
use cadmpeg_ir::transform::Transform;

use super::{
    decode_rigid_matrix, lp_ascii_strict, take_owned_reference, XrefReference, IDENTITY_ROWS,
};
use crate::layout::operation_table_header as header;
use crate::loss::F3dLossCode;
use crate::records::feature::assembly_features::DesignComponentOccurrence;

const ROOT_COMPONENT_TYPE_GUID: &str = "E03784ED-5E19-4E14-B9F2-3B07017018CD";
const TYPE_GUID: &str = "549FBB80-B890-473E-A5C0-415D3D9BF4E6";
const POSE_TRANSLATION_MM: f64 = 1.0e-4;
const POSE_ROTATION: f64 = 1.0e-9;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct OperationPose {
    pub(super) matrix: [[f64; 4]; 4],
    pub(super) frame: Vec<u8>,
    pub(super) carrier: u64,
    source_root_record_index: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DirectPose {
    pub(super) target_record_index: u64,
    pub(super) matrix: [[f64; 4]; 4],
    source_root_record_index: u64,
}

pub(super) fn parse(body: &[u8]) -> Option<DirectPose> {
    let mut matches = [2, 6]
        .into_iter()
        .filter_map(|prefix| parse_prefix(body, prefix));
    let result = matches.next()?;
    matches.next().is_none().then_some(result)
}

fn parse_prefix(body: &[u8], prefix: usize) -> Option<DirectPose> {
    class_tag(body)?;
    let (name, mut at) = lp_ascii_strict(body, header::LEN, 0..=body.len())?;
    if !name.is_ascii() {
        return None;
    }
    let end = at.checked_add(prefix)?;
    if body.get(at..end)?.iter().any(|b| *b != 0) {
        return None;
    }
    at = end;
    let deps = View::u32_le_at(body, at)?;
    at += 4;
    bounded_len(u64::from(deps), 1, body.len().checked_sub(at)?)?;
    for _ in 0..deps {
        take_owned_reference(body, &mut at)?;
    }
    if View::u32_le_at(body, at)? != 1 {
        return None;
    }
    at += 4;
    let (target, inline) = take_owned_reference(body, &mut at)?.into_local()?;
    if target == 0
        || inline
            .is_some_and(|guid| !guid.eq_ignore_ascii_case(super::OCCURRENCE_PLACEMENT_TYPE_GUID))
    {
        return None;
    }
    let (source_root_record_index, root_type) =
        take_owned_reference(body, &mut at)?.into_local()?;
    if source_root_record_index == 0
        || root_type.is_some_and(|guid| !guid.eq_ignore_ascii_case(ROOT_COMPONENT_TYPE_GUID))
    {
        return None;
    }
    let cursor = View::u32_le_at(body, at)?;
    let count = View::u32_le_at(body, at + 4)?;
    if count == 0 || cursor != count - 1 {
        return None;
    }
    at += 8;
    bounded_len(u64::from(count), 14, body.len().checked_sub(at)?)?;
    let mut matrix = IDENTITY_ROWS;
    for _ in 0..count {
        if !matches!(body.get(at..at + 2)?, [0 | 1, 0] | [1, 1]) {
            return None;
        }
        at += 2;
        take_owned_reference(body, &mut at)?.into_local()?;
        match body.get(at)? {
            1 => {
                matrix = IDENTITY_ROWS;
                at += 1;
            }
            0 => {
                matrix = decode_rigid_matrix(body, at + 1)?;
                at += 129;
            }
            _ => return None,
        }
    }
    (at == body.len()).then_some(DirectPose {
        target_record_index: target,
        matrix,
        source_root_record_index,
    })
}

fn class_tag(body: &[u8]) -> Option<&str> {
    if View::u32_le_at(body, header::CLASS_TAG_LENGTH)? != header::CLASS_TAG_LENGTH_VALUE {
        return None;
    }
    let tag = body.get(header::CLASS_TAG..header::RECORD_INDEX)?;
    if !tag.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(tag).ok()
}

pub(super) fn carrier_index(body: &[u8]) -> Option<u64> {
    View::u64_le_at(body, header::RECORD_INDEX)
}

#[derive(Default)]
struct OperationTables {
    poses: BTreeMap<(String, u64), Option<OperationPose>>,
    native: usize,
}

fn read(
    ctx: &DecodeContext<'_>,
    scan: &crate::container::ContainerScan<'_>,
) -> Result<OperationTables, CodecError> {
    let mut table = OperationTables::default();
    for entry in scan.entries.iter().filter(|entry| {
        scan.is_design_stream(entry, cadmpeg_core::container::ContainerRole::Bulkstream)
    }) {
        let Some(prefix) = entry.name.strip_suffix("BulkStream.dat") else {
            continue;
        };
        let meta_name = format!("{prefix}MetaStream.dat");
        if !scan.entries.iter().any(|entry| entry.name == meta_name) {
            continue;
        }
        let meta = scan.parsed_metastream(&meta_name)?;
        if !meta.types.iter().any(|entry| {
            entry.module == "Fusion" && entry.type_guid.as_str().eq_ignore_ascii_case(TYPE_GUID)
        }) {
            continue;
        }
        let mut types = BTreeMap::new();
        for (ordinal, ty) in meta.types.iter().enumerate().filter(|(_, ty)| {
            ty.module == "Fusion" && ty.type_guid.as_str().eq_ignore_ascii_case(TYPE_GUID)
        }) {
            for &entity in ty.entities.values() {
                types
                    .entry(entity)
                    .and_modify(|entry| {
                        if *entry != Some((ordinal, ty.version)) {
                            *entry = None;
                        }
                    })
                    .or_insert(Some((ordinal, ty.version)));
            }
        }
        let bytes = scan.entry_bytes(&entry.name)?;
        for record in crate::metastream::primary_record_frames(&meta, bytes.len())? {
            let Some(binding) = types.get(&record.entity_id) else {
                continue;
            };
            let Some((ordinal, version)) = binding else {
                table.native += 1;
                continue;
            };
            if *version > 1 {
                table.native += 1;
                continue;
            }
            let Some(body) = bytes.get(record.start..record.member_end) else {
                table.native += 1;
                continue;
            };
            let class = class_tag(body)
                .and_then(|tag| tag.parse::<usize>().ok())
                .and_then(|tag| tag.checked_sub(256));
            let identity = carrier_index(body);
            if class != Some(*ordinal) || identity != Some(record.entity_id) {
                table.native += 1;
                continue;
            }
            register(ctx, &mut table, &entry.name, body, Some(*version))?;
        }
    }
    Ok(table)
}

fn register(
    ctx: &DecodeContext<'_>,
    table: &mut OperationTables,
    stream: &str,
    body: &[u8],
    version: Option<u32>,
) -> Result<(), CodecError> {
    let pose = version.and_then(|version| {
        let prefix = match version {
            0 => 2,
            1 => 6,
            _ => return None,
        };
        parse(body).filter(|_| parse_prefix(body, prefix).is_some())
    });
    let Some(pose) = pose else {
        table.native += 1;
        return Ok(());
    };
    let carrier = carrier_index(body)
        .ok_or_else(|| CodecError::malformed("F3D operation table has no indexed identity"))?;
    let key = (crate::ids::native_scope(stream), pose.target_record_index);
    match table.poses.entry(key) {
        std::collections::btree_map::Entry::Vacant(entry) => {
            let mut frame = ctx.alloc_filled(body.len(), 0, "retain F3D operation-table pose")?;
            frame.copy_from_slice(body);
            entry.insert(Some(OperationPose {
                matrix: pose.matrix,
                frame,
                carrier,
                source_root_record_index: pose.source_root_record_index,
            }));
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            table.native += 1 + usize::from(entry.get().is_some());
            entry.insert(None);
        }
    }
    Ok(())
}

pub(crate) fn collect(
    ctx: &DecodeContext<'_>,
    scan: &crate::container::ContainerScan<'_>,
    ir: &cadmpeg_ir::CadIr,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<BTreeMap<String, OperationPose>, CodecError> {
    let mut result = BTreeMap::new();
    if ctx.container_only() {
        return Ok(result);
    }
    let mut matched = BTreeSet::new();
    let mut table = read(ctx, scan)?;
    if table.native != 0 {
        losses.push(F3dLossCode::OperationPoseNative.note(format!("{} operation table(s) have no unique admitted direct occurrence frame; they stay native", table.native)));
    }
    if table.poses.values().all(Option::is_none) {
        return Ok(result);
    }
    let Some(ns) = ir.native.namespace("f3d") else {
        losses.push(
            F3dLossCode::OperationPoseNative.note(
                "direct operation tables have no native occurrence namespace; they stay native",
            ),
        );
        return Ok(result);
    };
    let spaces = ns
        .arena_iter_as_for_decode::<crate::records::recipes::DesignComponentNamingSpace>(
            ctx,
            "design_component_naming_spaces",
        )
        .collect::<Result<Vec<_>, _>>()?;
    let roots = crate::design::decode::components::decode_local_components(scan, &spaces)?
        .into_iter()
        .filter_map(|(stream, components)| {
            components
                .root_component
                .filter(|root| components.roots.contains_key(root))
                .map(|root| (stream, root))
        })
        .collect();
    bind_source_roots(&mut table, &roots, losses);
    for record in ns.arena_iter_as_for_decode::<XrefReference>(ctx, "xref_references") {
        let r = record?;
        if let Some(source) = &r.placement_source {
            matched.insert((source.stream.clone(), source.carrier_record_index));
        }
        // Carriers of a nested design place their instance after the operation
        // tables, and yield to its table when they do not agree.
        let contained = r
            .contained_poses
            .iter()
            .any(|pose| !super::contained_pose_is_nested(pose));
        if r.current_pose.is_some() || contained || !r.owned_poses.is_empty() {
            if r.placement_source.as_ref().is_some_and(|s| {
                table
                    .poses
                    .contains_key(&(s.stream.clone(), s.carrier_record_index))
            }) {
                losses.push(F3dLossCode::OperationPoseNative.note(format!("occurrence {}: component-path selectors take precedence over its operation table", r.id)));
            }
            continue;
        }
        if let Some(source) = r.placement_source {
            if let Some(Some(pose)) = table
                .poses
                .get(&(source.stream, source.carrier_record_index))
            {
                result.insert(r.id, pose.clone());
            }
        }
    }
    for record in ns
        .arena_iter_as_for_decode::<DesignComponentOccurrence>(ctx, "design_component_occurrences")
    {
        let r = record?;
        let Some(stream) = crate::ids::native_stream(&r.id) else {
            continue;
        };
        matched.insert((stream.to_owned(), u64::from(r.record_index)));
        if r.current_pose.is_some() {
            if table
                .poses
                .contains_key(&(stream.to_owned(), u64::from(r.record_index)))
            {
                losses.push(F3dLossCode::OperationPoseNative.note(format!("occurrence {}: its local component-path selector takes precedence over its operation table", r.id)));
            }
            continue;
        }
        if let Some(Some(pose)) = table
            .poses
            .get(&(stream.to_owned(), u64::from(r.record_index)))
        {
            result.insert(r.id, pose.clone());
        }
    }
    for ((stream, target), pose) in &table.poses {
        if pose.is_some() && !matched.contains(&(stream.clone(), *target)) {
            losses.push(F3dLossCode::OperationPoseNative.note(format!("operation table for {stream} occurrence {target}: no native occurrence binds its path; it stays native")));
        }
    }
    Ok(result)
}

fn bind_source_roots(
    table: &mut OperationTables,
    roots: &BTreeMap<String, u64>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) {
    for ((stream, target), pending) in &mut table.poses {
        let Some(pose) = pending else { continue };
        if roots.get(stream).copied() != Some(pose.source_root_record_index) {
            losses.push(F3dLossCode::OperationPoseNative.note(format!("occurrence {target} in {stream}: the table's component reference does not bind the source root frame; the table stays native")));
            *pending = None;
        }
    }
}

pub(crate) fn reference_matches(reference: &XrefReference) -> bool {
    let Some(pose) = &reference.operation_pose else {
        return true;
    };
    reference
        .placement_source
        .as_ref()
        .is_some_and(|source| matches_pose(pose, source.carrier_record_index))
}

pub(crate) fn local_matches(occurrence: &DesignComponentOccurrence) -> bool {
    occurrence
        .operation_pose
        .as_ref()
        .is_none_or(|pose| matches_pose(pose, u64::from(occurrence.record_index)))
}

fn matches_pose(pose: &crate::records::xref::XrefOperationPose, target: u64) -> bool {
    parse(&pose.frame_bytes).is_some_and(|parsed| {
        parsed.target_record_index == target
            && carrier_index(&pose.frame_bytes) == Some(pose.carrier_record_index)
            && pose.transform.map_or(
                IDENTITY_ROWS,
                crate::records::xref::XrefPlacementTransform::rows,
            ) == parsed.matrix
    })
}

fn agree(a: Transform, b: Transform) -> bool {
    a.rows()
        .iter()
        .flatten()
        .zip(b.rows().iter().flatten())
        .enumerate()
        .all(|(i, (a, b))| {
            (a - b).abs()
                <= if i % 4 == 3 {
                    POSE_TRANSLATION_MM
                } else {
                    POSE_ROTATION
                }
        })
}

struct Placement<'a> {
    occurrences: &'a [Occurrence],
    index: BTreeMap<&'a str, usize>,
    candidates: BTreeMap<usize, (Option<usize>, Transform)>,
    protected: &'a BTreeSet<cadmpeg_ir::ids::OccurrenceId>,
}

impl Placement<'_> {
    fn occurrence(&self, i: usize) -> Result<&Occurrence, CodecError> {
        self.occurrences
            .get(i)
            .ok_or_else(|| CodecError::malformed("F3D operation pose has no occurrence"))
    }

    fn parent(&self, i: usize) -> Result<Option<usize>, CodecError> {
        match &self.occurrence(i)?.parent {
            OccurrenceParent::Occurrence { occurrence } => self
                .index
                .get(occurrence.as_str())
                .copied()
                .map(Some)
                .ok_or_else(|| CodecError::malformed("F3D operation pose has no owner")),
            OccurrenceParent::Root {} => Ok(None),
        }
    }

    fn world(&self, mut i: usize, mut baseline: bool) -> Result<Transform, CodecError> {
        let mut chain = Vec::new();
        loop {
            if chain.len() > self.occurrences.len() {
                return Err(CodecError::malformed("F3D operation pose owner cycle"));
            }
            let o = self.occurrence(i)?;
            baseline |= self.protected.contains(&o.id);
            let (parent, local) = if baseline {
                (self.parent(i)?, o.transform)
            } else {
                self.candidates
                    .get(&i)
                    .copied()
                    .unwrap_or((self.parent(i)?, o.transform))
            };
            chain.push(local);
            let Some(parent) = parent else { break };
            i = parent;
        }
        compose_chain(chain)
    }

    fn candidate_world(&self, i: usize) -> Result<Transform, CodecError> {
        let (parent, local) = self
            .candidates
            .get(&i)
            .copied()
            .unwrap_or((self.parent(i)?, self.occurrence(i)?.transform));
        let parent = parent.map_or(Ok(Transform::identity()), |parent| {
            self.world(parent, false)
        })?;
        parent
            .compose(local)
            .map_err(|_| CodecError::malformed("F3D operation pose overflow"))
    }

    fn placed_world(&self, mut i: usize) -> Result<Transform, CodecError> {
        let mut chain = Vec::new();
        loop {
            if chain.len() > self.occurrences.len() {
                return Err(CodecError::malformed("F3D operation pose owner cycle"));
            }
            chain.push(if self.candidates.contains_key(&i) {
                self.local(i)?
            } else {
                self.occurrence(i)?.transform
            });
            let Some(parent) = self.parent(i)? else { break };
            i = parent;
        }
        compose_chain(chain)
    }

    fn local(&self, i: usize) -> Result<Transform, CodecError> {
        let parent = match self.parent(i)? {
            Some(j) => self.world(j, false)?,
            None => Transform::identity(),
        };
        let target = self.world(i, false)?;
        parent
            .try_inverse_affine()
            .and_then(|inverse| inverse.compose(target))
            .map_err(|_| CodecError::malformed("F3D operation pose inverse"))
    }
}

fn compose_chain(chain: Vec<Transform>) -> Result<Transform, CodecError> {
    chain
        .into_iter()
        .rev()
        .try_fold(Transform::identity(), |world, local| {
            world
                .compose(local)
                .map_err(|_| CodecError::malformed("F3D operation pose overflow"))
        })
}

pub(crate) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut cadmpeg_ir::CadIr,
    poses: &BTreeMap<String, OperationPose>,
    protected: &BTreeSet<cadmpeg_ir::ids::OccurrenceId>,
    losses: &mut Vec<cadmpeg_ir::report::loss::LossNote>,
) -> Result<usize, CodecError> {
    if poses.is_empty() {
        return Ok(0);
    }
    let Some(ns) = ir.native.namespace("f3d") else {
        return Ok(0);
    };
    let mut streams = BTreeMap::new();
    for r in ns.arena_iter_as_for_decode::<XrefReference>(ctx, "xref_references") {
        let r = r?;
        if let Some(s) = r.placement_source {
            streams.insert(r.id, s.stream);
        }
    }
    for r in ns
        .arena_iter_as_for_decode::<DesignComponentOccurrence>(ctx, "design_component_occurrences")
    {
        let r = r?;
        if let Some(s) = crate::ids::native_stream(&r.id) {
            streams.insert(r.id.clone(), s.to_owned());
        }
    }
    let mut placement = Placement {
        occurrences: &ir.model.occurrences,
        index: ir
            .model
            .occurrences
            .iter()
            .enumerate()
            .map(|(i, o)| (o.id.as_str(), i))
            .collect(),
        candidates: BTreeMap::new(),
        protected,
    };
    let mut groups = BTreeMap::<&str, Vec<usize>>::new();
    for (i, o) in placement.occurrences.iter().enumerate() {
        let Some(key) = o
            .native_ref
            .as_deref()
            .filter(|key| poses.contains_key(*key))
        else {
            continue;
        };
        let Some(stream) = streams.get(key) else {
            continue;
        };
        let Some(pose) = poses.get(key) else { continue };
        let mut boundary = None;
        let mut at = placement.parent(i)?;
        let mut seen = BTreeSet::new();
        while let Some(j) = at {
            if !seen.insert(j) {
                return Err(CodecError::malformed("F3D operation pose owner cycle"));
            }
            if placement
                .occurrence(j)?
                .native_ref
                .as_ref()
                .and_then(|r| streams.get(r))
                .is_some_and(|s| s != stream)
            {
                boundary = Some(j);
                break;
            }
            at = placement.parent(j)?;
        }
        groups.entry(key).or_default().push(i);
        placement.candidates.insert(
            i,
            (
                boundary,
                crate::design::components::neutral_transform(pose.matrix)?,
            ),
        );
    }
    let mut refused = BTreeSet::new();
    loop {
        let mut newly_refused = Vec::new();
        for (&key, instances) in &groups {
            if refused.contains(key) {
                continue;
            }
            let mut first = None;
            for &i in instances {
                let local = placement.local(i)?;
                let o = placement.occurrence(i)?;
                if (protected.contains(&o.id)
                    && !agree(placement.candidate_world(i)?, placement.world(i, true)?))
                    || first.is_some_and(|first| !agree(first, local))
                {
                    newly_refused.push(key);
                    break;
                }
                first = Some(local);
            }
        }
        for id in protected {
            let Some(&i) = placement.index.get(id.as_str()) else {
                continue;
            };
            if agree(placement.placed_world(i)?, placement.world(i, true)?) {
                continue;
            }
            let mut at = Some(i);
            let mut seen = BTreeSet::new();
            while let Some(j) = at {
                if !seen.insert(j) {
                    return Err(CodecError::malformed("F3D operation pose owner cycle"));
                }
                if placement.candidates.contains_key(&j) {
                    if let Some(key) = placement.occurrence(j)?.native_ref.as_deref() {
                        if let Some((&key, _)) = groups.get_key_value(key) {
                            newly_refused.push(key);
                        }
                    }
                }
                at = placement.parent(j)?;
            }
        }
        if newly_refused.is_empty() {
            break;
        }
        for key in newly_refused {
            refused.insert(key);
            if let Some(instances) = groups.get(key) {
                for i in instances {
                    placement.candidates.remove(i);
                }
            }
        }
    }
    let mut placed = Vec::new();
    let mut applied = BTreeSet::new();
    for (&key, instances) in &groups {
        if refused.contains(key) {
            losses.push(F3dLossCode::OperationPoseDisagrees.note(format!("occurrence {key}: operation-table pose cannot preserve the component-path constraints for every instance; the definition keeps its stored pose")));
            continue;
        }
        let Some(&first) = instances.first() else {
            continue;
        };
        let local = placement.local(first)?;
        for &i in instances {
            placed.push((i, local));
        }
        applied.insert(key.to_owned());
    }
    for key in poses
        .keys()
        .filter(|key| !groups.contains_key(key.as_str()))
    {
        losses.push(F3dLossCode::OperationPoseNative.note(format!(
            "occurrence {key}: operation table names no projected occurrence; it stays native"
        )));
    }
    let count = placed
        .iter()
        .filter(|(i, transform)| {
            ir.model
                .occurrences
                .get(*i)
                .is_some_and(|o| !agree(*transform, o.transform))
        })
        .count();
    for (i, transform) in placed {
        if let Some(o) = ir.model.occurrences.get_mut(i) {
            o.transform = transform;
        }
    }
    let ns = ir.native.namespace_mut("f3d");
    for name in ["xref_references", "design_component_occurrences"] {
        if let Some(records) = ns.arenas_mut().get_mut(name) {
            for r in records {
                if !applied.contains(r.id()) {
                    continue;
                }
                let Some(pose) = poses.get(r.id()) else {
                    continue;
                };
                let operation = crate::records::xref::XrefOperationPose {
                    carrier_record_index: pose.carrier,
                    frame_bytes: pose.frame.clone().into(),
                    transform: Some(pose.matrix.try_into().map_err(CodecError::NotImplemented)?),
                };
                let mut fields = r.copy_fields(ctx)?;
                fields.insert(
                    "operation_pose".into(),
                    serde_json::to_value(operation)
                        .map_err(|e| CodecError::malformed(e.to_string()))?,
                );
                *r = cadmpeg_ir::NativeRecord::new(
                    cadmpeg_ir::ids::Identity::new(r.id().to_owned())
                        .map_err(CodecError::malformed)?,
                    fields,
                )?;
            }
        }
    }
    if !applied.is_empty() {
        losses.push(F3dLossCode::OperationPoseApplied.note(format!("{} occurrence definition(s) take their direct operation table's final pose in every instance; {count} instance transform(s) changed", applied.len())));
    }
    Ok(count)
}

#[cfg(test)]
mod tests;
