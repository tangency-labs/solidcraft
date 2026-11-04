// SPDX-License-Identifier: Apache-2.0
//! Decode `TSplines.BlobParts/*.tsm` Form control cages.

use cadmpeg_core::decode::index_from_u32;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, NonNegativeReal, PositiveReal};
use cadmpeg_ir::subd::{
    SubdEdge, SubdEdgeTag, SubdEdgeUse, SubdFace, SubdGripDirection, SubdGripWedge, SubdPlaneFrame,
    SubdRadialMapSelector, SubdRadialSymmetryMap, SubdScheme, SubdSecondaryGrip, SubdSurface,
    SubdSymmetry, SubdSymmetryKind, SubdUserKnot, SubdVertex, SubdVertexGripLayout, SubdVertexTag,
};
use cadmpeg_ir::SourceObjectAssociation;

use crate::container::ContainerScan;
use crate::loss::F3dLossCode;

pub(crate) mod user_knots;

const ENTRY_MARKER: &str = "/TSplines.BlobParts/";
const CAGE_COORDINATE_SCALE: f64 = 10.0;
const FULL_CREASE_SHARPNESS: f64 = 1.0;
const SYMMETRY_FRAME_EPS: f64 = 1.0e-9;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct HalfEdgeId(usize);

#[derive(Clone, Copy)]
struct HalfEdge {
    next: HalfEdgeId,
    previous: HalfEdgeId,
    mate: HalfEdgeId,
    vertex: usize,
    face: Option<usize>,
}

impl HalfEdgeId {
    fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy)]
struct ParsedHalfEdge {
    next: usize,
    previous: usize,
    mate: usize,
    vertex: usize,
    face: i64,
}

fn compact_half_edges(
    ctx: &DecodeContext<'_>,
    name: &str,
    slots: &[Option<ParsedHalfEdge>],
    face_roots: &mut [Option<usize>],
    edge_roots: &mut [Option<usize>],
    vertex_roots: &mut [Option<(usize, SubdGripDirection)>],
) -> Result<Vec<HalfEdge>, CodecError> {
    let mut map = ctx.alloc_filled(slots.len(), None, "map T-spline half-edge slots")?;
    let mut dense = Vec::new();
    for (old, half) in slots.iter().enumerate() {
        if let Some(half) = half {
            map[old] = Some(HalfEdgeId(dense.len()));
            dense.push(*half);
        }
    }
    let remap = |index: usize| {
        map.get(index)
            .copied()
            .flatten()
            .ok_or_else(|| malformed(name, "half-edge names a deleted slot"))
    };
    let mut half_edges = Vec::new();
    for half in dense {
        let resolved = HalfEdge {
            next: remap(half.next)?,
            previous: remap(half.previous)?,
            mate: remap(half.mate)?,
            vertex: half.vertex,
            face: match half.face {
                -1 => None,
                face => Some(
                    usize::try_from(face)
                        .map_err(|_| malformed(name, "half-edge face is negative or overflows"))?,
                ),
            },
        };
        half_edges.push(resolved);
    }
    for root in face_roots.iter_mut().flatten() {
        *root = remap(*root)?.index();
    }
    for root in edge_roots.iter_mut().flatten() {
        *root = remap(*root)?.index();
    }
    for root in vertex_roots.iter_mut().flatten() {
        root.0 = remap(root.0)?.index();
    }
    Ok(half_edges)
}

struct UntypedRecords<'a>(&'a BTreeMap<String, usize>);

impl std::fmt::Display for UntypedRecords<'_> {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let count = self.0.values().sum::<usize>();
        write!(
            out,
            "{count} T-spline record(s) were retained without typed semantics: "
        )?;
        for (index, (kind, count)) in self.0.iter().enumerate() {
            if index != 0 {
                out.write_str(", ")?;
            }
            write!(out, "{kind}={count}")?;
        }
        out.write_str(".")
    }
}

#[derive(Clone, Copy)]
struct GripPoint {
    point: Point3,
    weight: PositiveReal,
}

#[derive(Clone, Copy)]
enum GripVertexMarker {
    Primary(usize),
    Secondary(Option<usize>),
}

/// Decode every active-asset T-spline control cage in archive order.
///
/// A cage whose program is internally inconsistent degrades to an
/// error-severity loss note instead of failing the document decode; its
/// entry bytes remain retained in the container, and the serializer-backed
/// Form join leaves the affected Form on native retention.
pub(crate) fn decode(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<(Vec<SubdSurface>, Vec<cadmpeg_ir::report::loss::LossNote>), CodecError> {
    let Some(folder) = scan.design_asset_folder() else {
        return Ok((Vec::new(), Vec::new()));
    };
    let mut cages = Vec::new();
    let mut losses = Vec::new();
    for entry in scan.entries.iter().filter(|entry| {
        std::path::Path::new(&entry.name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("tsm"))
            && entry
                .name
                .strip_prefix(folder)
                .is_some_and(|relative| relative.starts_with(ENTRY_MARKER))
    }) {
        match parse(ctx, &entry.name, scan.entry_bytes(&entry.name)?) {
            Ok(parsed) => {
                if !parsed.unknown_record_kinds.is_empty() {
                    let message = format!("{}", UntypedRecords(&parsed.unknown_record_kinds));
                    losses.push(F3dLossCode::TsplineRecordUntyped.note(message));
                }
                if parsed.unresolved_radial_pairs != 0 {
                    losses.push(F3dLossCode::TsplineRadialMapUnresolved.note(format!(
                        "{} radial symmetry pair(s) retain native identifiers without an unambiguous topology correspondence.",
                        parsed.unresolved_radial_pairs
                    )));
                }
                cages.push(parsed.surface);
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                let message = format!("T-spline control cage not decoded: {error}");
                losses.push(F3dLossCode::TsplineCageUndecoded.note(message));
            }
        }
    }
    Ok((cages, losses))
}

fn malformed(name: &str, message: impl std::fmt::Display) -> CodecError {
    CodecError::malformed(format_args!("T-spline cage {name}: {message}"))
}

fn subd_id(name: &str, source_key: &str) -> Result<cadmpeg_ir::ids::SubdId, CodecError> {
    let key_text = source_key.to_owned();
    let key = cadmpeg_ir::ids::IdentityKey::try_new(key_text)
        .map_err(|error| malformed(name, format_args!("invalid subd identity: {error}")))?;
    let text = format!("f3d:tspline:subd#{}", key.as_str());
    cadmpeg_ir::ids::SubdId::mint(text)
        .map_err(|error| malformed(name, format_args!("invalid subd identity: {error}")))
}

fn parse_int<T: std::str::FromStr>(
    name: &str,
    value: Option<&str>,
    field: &str,
) -> Result<T, CodecError> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| malformed(name, format_args!("invalid {field}")))
}

fn parse_f64(name: &str, value: Option<&str>, field: &str) -> Result<FiniteReal, CodecError> {
    value
        .and_then(|value| value.parse().ok())
        .and_then(FiniteReal::new)
        .ok_or_else(|| malformed(name, format_args!("invalid {field}")))
}

fn parse_direction(name: &str, value: Option<&str>) -> Result<SubdGripDirection, CodecError> {
    match value {
        Some("NORTH") => Ok(SubdGripDirection::North),
        Some("EAST") => Ok(SubdGripDirection::East),
        Some("SOUTH") => Ok(SubdGripDirection::South),
        Some("WEST") => Ok(SubdGripDirection::West),
        _ => Err(malformed(name, "invalid vertex direction")),
    }
}

/// Map each program slot to its IR index, or `None` for a deleted slot.
fn compact(live: impl Iterator<Item = bool>) -> Result<Vec<Option<u32>>, CodecError> {
    let mut next = 0u32;
    let mut compacted = Vec::new();
    for live in live {
        let index = if live {
            let index = next;
            next = next
                .checked_add(1)
                .ok_or_else(|| CodecError::malformed("T-spline live slot count exceeds u32"))?;
            Some(index)
        } else {
            None
        };
        compacted.push(index);
    }
    Ok(compacted)
}

fn require_end<'a>(
    name: &str,
    mut fields: impl Iterator<Item = &'a str>,
    record: &str,
) -> Result<(), CodecError> {
    if fields.next().is_some() {
        return Err(malformed(
            name,
            format_args!("{record} has trailing fields"),
        ));
    }
    Ok(())
}

#[derive(Debug)]
struct ParsedCage {
    surface: SubdSurface,
    unknown_record_kinds: BTreeMap<String, usize>,
    unresolved_radial_pairs: usize,
}

#[derive(Debug)]
struct DerivedGripConnectivity {
    vertex: usize,
    spoke_lengths: Vec<usize>,
    grip_indices: Vec<Option<usize>>,
}

#[derive(Clone, Copy)]
enum FanSlot {
    Phantom,
    Slot {
        half_edge: usize,
        face: Option<usize>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SymmetryMode {
    Correspondence,
    Radial,
}

#[derive(Debug)]
struct PartialSymmetryBlock {
    mode: SymmetryMode,
    plane: Option<[FiniteReal; 12]>,
    radial_segments: Option<std::num::NonZeroU32>,
    radial_sweep: Option<FiniteReal>,
    radial_maps: Vec<SubdRadialSymmetryMap>,
    record_kinds: BTreeSet<String>,
    face_forward: BTreeMap<usize, usize>,
    face_reverse: BTreeMap<usize, usize>,
    edge_forward: BTreeMap<usize, usize>,
    edge_reverse: BTreeMap<usize, usize>,
    vertex_forward: BTreeMap<usize, usize>,
    vertex_reverse: BTreeMap<usize, usize>,
}

impl PartialSymmetryBlock {
    fn new(mode: SymmetryMode) -> Self {
        Self {
            mode,
            plane: None,
            radial_segments: None,
            radial_sweep: None,
            radial_maps: Vec::new(),
            record_kinds: BTreeSet::new(),
            face_forward: BTreeMap::new(),
            face_reverse: BTreeMap::new(),
            edge_forward: BTreeMap::new(),
            edge_reverse: BTreeMap::new(),
            vertex_forward: BTreeMap::new(),
            vertex_reverse: BTreeMap::new(),
        }
    }
}

#[derive(Debug)]
struct SymmetryBlock {
    plane: [FiniteReal; 12],
    kind: SymmetryKind,
}

#[derive(Debug)]
enum SymmetryKind {
    Correspondence {
        face: BTreeMap<usize, usize>,
        edge: BTreeMap<usize, usize>,
        vertex: BTreeMap<usize, usize>,
    },
    Radial {
        segments: std::num::NonZeroU32,
        sweep: FiniteReal,
        maps: Vec<SubdRadialSymmetryMap>,
    },
}

#[derive(Default)]
struct ProjectedRadialPairs {
    face: Vec<[u32; 2]>,
    edge: Vec<[u32; 2]>,
    vertex: Vec<[u32; 2]>,
    unresolved: usize,
}

fn agreed_radial_index(id: u64, slots: &[Option<u32>], live_count: usize) -> Option<u32> {
    let index = usize::try_from(id).ok()?;
    let raw_slot = slots.get(index).copied().flatten()?;
    let compact_index = u32::try_from(id).ok()?;
    (index < live_count && raw_slot == compact_index).then_some(raw_slot)
}

fn project_radial_pairs(pairs: &[[u64; 2]], slots: &[Option<u32>]) -> (Vec<[u32; 2]>, usize) {
    let live_count = slots.iter().flatten().count();
    let candidates: Vec<Option<[u32; 2]>> = pairs
        .iter()
        .map(|[source, target]| {
            Some([
                agreed_radial_index(*source, slots, live_count)?,
                agreed_radial_index(*target, slots, live_count)?,
            ])
        })
        .collect();
    let mut targets = BTreeMap::new();
    for pair in candidates.iter().flatten() {
        *targets.entry(pair[1]).or_insert(0usize) += 1;
    }
    let mut resolved = Vec::new();
    let mut unresolved = 0;
    for pair in candidates {
        match pair {
            Some(pair) if targets.get(&pair[1]) == Some(&1) => resolved.push(pair),
            _ => unresolved += 1,
        }
    }
    (resolved, unresolved)
}

fn mutual_radial_pairs(
    forward: Vec<[u32; 2]>,
    reverse: &[[u32; 2]],
    unresolved: &mut usize,
) -> Vec<[u32; 2]> {
    let forward_set: BTreeSet<_> = forward.iter().copied().collect();
    let reverse_set: BTreeSet<_> = reverse.iter().copied().collect();
    *unresolved += reverse
        .iter()
        .filter(|[source, target]| !forward_set.contains(&[*target, *source]))
        .count();
    forward
        .into_iter()
        .filter(|[source, target]| {
            if reverse_set.contains(&[*target, *source]) {
                true
            } else {
                *unresolved += 1;
                false
            }
        })
        .collect()
}

fn project_radial_maps(
    maps: &[SubdRadialSymmetryMap],
    faces: &[Option<u32>],
    edges: &[Option<u32>],
    vertices: &[Option<u32>],
) -> ProjectedRadialPairs {
    let mut projected = ProjectedRadialPairs::default();
    let mut reverse_face = Vec::new();
    let mut reverse_edge = Vec::new();
    let mut reverse_vertex = Vec::new();
    for map in maps {
        let slots = match map.selector {
            SubdRadialMapSelector::Ef | SubdRadialMapSelector::Er => edges,
            SubdRadialMapSelector::Ff | SubdRadialMapSelector::Fr => faces,
            SubdRadialMapSelector::Vf | SubdRadialMapSelector::Vr => vertices,
        };
        let (pairs, unresolved) = project_radial_pairs(&map.pairs, slots);
        projected.unresolved += unresolved;
        match map.selector {
            SubdRadialMapSelector::Ef => projected.edge = pairs,
            SubdRadialMapSelector::Ff => projected.face = pairs,
            SubdRadialMapSelector::Vf => projected.vertex = pairs,
            SubdRadialMapSelector::Er => reverse_edge = pairs,
            SubdRadialMapSelector::Fr => reverse_face = pairs,
            SubdRadialMapSelector::Vr => reverse_vertex = pairs,
        }
    }
    projected.face = mutual_radial_pairs(projected.face, &reverse_face, &mut projected.unresolved);
    projected.edge = mutual_radial_pairs(projected.edge, &reverse_edge, &mut projected.unresolved);
    projected.vertex =
        mutual_radial_pairs(projected.vertex, &reverse_vertex, &mut projected.unresolved);
    projected
}

fn parse_pairs<'a>(
    name: &str,
    fields: impl Iterator<Item = &'a str>,
    record: &str,
) -> Result<BTreeMap<usize, usize>, CodecError> {
    let mut values = Vec::new();
    for value in fields {
        values.push(parse_int::<usize>(name, Some(value), record)?);
    }
    if values.len() % 2 != 0 {
        return Err(malformed(
            name,
            format_args!("{record} has an unpaired index"),
        ));
    }
    let mut pairs = BTreeMap::new();
    for pair in values.chunks_exact(2) {
        if pairs.contains_key(&pair[0]) {
            return Err(malformed(
                name,
                format_args!("{record} repeats a source index"),
            ));
        }
        pairs.insert(pair[0], pair[1]);
    }
    Ok(pairs)
}

fn parse_radial_pairs<'a>(
    name: &str,
    fields: impl Iterator<Item = &'a str>,
) -> Result<Vec<[u64; 2]>, CodecError> {
    let mut values = Vec::new();
    for value in fields {
        let index = value
            .parse::<u64>()
            .map_err(|_| malformed(name, "invalid radial symmetry map index"))?;
        values.push(index);
    }
    if values.len() % 2 != 0 {
        return Err(malformed(name, "radial symmetry map has an unpaired index"));
    }
    let mut sources = BTreeSet::new();
    let mut pairs = Vec::new();
    for pair in values.chunks_exact(2) {
        if sources.contains(&pair[0]) {
            return Err(malformed(
                name,
                "radial symmetry map repeats a source index",
            ));
        }
        sources.insert(pair[0]);
        pairs.push([pair[0], pair[1]]);
    }
    Ok(pairs)
}

fn validate_symmetry_map(
    name: &str,
    forward: &BTreeMap<usize, usize>,
    reverse: &BTreeMap<usize, usize>,
    slots: &[bool],
    element: &str,
) -> Result<(), CodecError> {
    for (&source, &target) in forward {
        if !slots.get(source).copied().unwrap_or(false)
            || !slots.get(target).copied().unwrap_or(false)
            || (source != target && reverse.get(&target) != Some(&source))
        {
            return Err(malformed(
                name,
                format_args!("{element} symmetry map is inconsistent"),
            ));
        }
    }
    for (&source, &target) in reverse {
        if !slots.get(source).copied().unwrap_or(false)
            || !slots.get(target).copied().unwrap_or(false)
            || forward.get(&target) != Some(&source)
        {
            return Err(malformed(
                name,
                format_args!("{element} symmetry map is inconsistent"),
            ));
        }
    }
    Ok(())
}

fn symmetry_plane(name: &str, values: [FiniteReal; 12]) -> Result<SubdPlaneFrame, CodecError> {
    let origin = Point3::new(
        values[0].get() * CAGE_COORDINATE_SCALE,
        values[1].get() * CAGE_COORDINATE_SCALE,
        values[2].get() * CAGE_COORDINATE_SCALE,
    );
    let first_axis = Vector3::new(values[4].get(), values[5].get(), values[6].get());
    let second_axis = Vector3::new(values[8].get(), values[9].get(), values[10].get());
    if (values[3].get() - 1.0).abs() > SYMMETRY_FRAME_EPS
        || values[7].get().abs() > SYMMETRY_FRAME_EPS
        || values[11].get().abs() > SYMMETRY_FRAME_EPS
    {
        return Err(malformed(
            name,
            "symmetry plane is not a homogeneous orthonormal frame",
        ));
    }
    SubdPlaneFrame::new(origin, first_axis, second_axis).map_err(|error| {
        malformed(
            name,
            format_args!("symmetry plane is not a homogeneous orthonormal frame: {error}"),
        )
    })
}

fn remap_symmetry_pairs(
    name: &str,
    map: &BTreeMap<usize, usize>,
    ir_indices: &[Option<u32>],
    element: &str,
) -> Result<Vec<[u32; 2]>, CodecError> {
    let mut remapped = Vec::new();
    for (&source, &target) in map {
        let source =
            ir_indices.get(source).copied().flatten().ok_or_else(|| {
                malformed(name, format_args!("{element} symmetry source is deleted"))
            })?;
        let target =
            ir_indices.get(target).copied().flatten().ok_or_else(|| {
                malformed(name, format_args!("{element} symmetry target is deleted"))
            })?;
        remapped.push([source, target]);
    }
    Ok(remapped)
}

fn direction_offset(direction: SubdGripDirection) -> usize {
    match direction {
        SubdGripDirection::North => 0,
        SubdGripDirection::East => 1,
        SubdGripDirection::South => 2,
        SubdGripDirection::West => 3,
    }
}

fn build_fan(
    name: &str,
    vertex: usize,
    root: usize,
    half_edges: &[HalfEdge],
    face_live: &[bool],
) -> Result<Vec<FanSlot>, CodecError> {
    let root_id = HalfEdgeId(root);
    let root_half = &half_edges[root];
    if root_half.vertex != vertex {
        return Err(malformed(
            name,
            "vertex root does not terminate at its vertex",
        ));
    }

    let mut fan = Vec::new();
    let mut seen = BTreeSet::new();
    let mut current = root_id;
    loop {
        if seen.contains(&current) {
            if current == root_id {
                break;
            }
            return Err(malformed(
                name,
                "vertex half-edge fan repeats before its root",
            ));
        }
        seen.insert(current);
        let half = &half_edges[current.index()];
        if half.vertex != vertex {
            return Err(malformed(
                name,
                "vertex half-edge fan leaves its terminal vertex",
            ));
        }
        if half
            .face
            .is_some_and(|face| !face_live.get(face).copied().unwrap_or(false))
        {
            return Err(malformed(
                name,
                "vertex half-edge fan names an invalid face",
            ));
        }
        fan.push(FanSlot::Slot {
            half_edge: current.index(),
            face: half.face,
        });

        let next = &half_edges[half.next.index()];
        let mate = &half_edges[next.mate.index()];
        let Some(rotated) = (mate.vertex == vertex).then_some(next.mate) else {
            return Err(malformed(
                name,
                "vertex fan rotation leaves its terminal vertex",
            ));
        };
        current = rotated;
        if current == root_id {
            break;
        }
        if seen.len() >= half_edges.len() {
            return Err(malformed(name, "vertex half-edge fan does not close"));
        }
    }

    let mut gap = None;
    for (index, slot) in fan.iter().enumerate() {
        if matches!(slot, FanSlot::Slot { face: None, .. }) && gap.replace(index).is_some() {
            return Err(malformed(
                name,
                "vertex half-edge fan has multiple boundary gaps",
            ));
        }
    }
    if let Some(gap) = gap {
        let phantom_count = if fan.len() < 4 { 4 - fan.len() } else { 0 };

        for _ in 0..phantom_count {
            fan.insert(gap + 1, FanSlot::Phantom);
        }
    }
    Ok(fan)
}

fn grip_block(
    name: &str,
    grip_points: &[Option<GripPoint>],
    indices: &[Option<usize>],
    cursor: &mut usize,
    count: usize,
) -> Result<Vec<Option<SubdSecondaryGrip>>, CodecError> {
    let end = cursor
        .checked_add(count)
        .ok_or_else(|| malformed(name, "derived-grip block arity overflows"))?;
    let values = indices
        .get(*cursor..end)
        .ok_or_else(|| malformed(name, "derived-grip run is shorter than its declared arity"))?;
    *cursor = end;
    let mut grips = Vec::new();
    for &index in values {
        let grip = index
            .map(|index| {
                let point =
                    grip_points.get(index).copied().flatten().ok_or_else(|| {
                        malformed(name, "derived-grip entry names a deleted grip")
                    })?;
                SubdSecondaryGrip::from_parts(
                    u32::try_from(index)
                        .map_err(|_| malformed(name, "secondary grip index overflows IR"))?,
                    point.point,
                    point.weight,
                )
                .map_err(|error| malformed(name, error))
            })
            .transpose()?;
        grips.push(grip);
    }
    Ok(grips)
}

struct SecondaryLayoutContext<'a> {
    name: &'a str,
    vertex_roots: &'a [Option<(usize, SubdGripDirection)>],
    vertex_live: &'a [bool],
    vertex_ir: &'a [Option<u32>],
    face_live: &'a [bool],
    face_ir: &'a [Option<u32>],
    half_edges: &'a [HalfEdge],
    edge_by_half: &'a [Option<(u32, bool)>],
    grip_vertices: &'a [GripVertexMarker],
    grip_points: &'a [Option<GripPoint>],
}

fn build_secondary_layouts(
    ctx: &DecodeContext<'_>,
    context: &SecondaryLayoutContext<'_>,
    derived_grips: &[DerivedGripConnectivity],
) -> Result<Vec<Option<SubdVertexGripLayout>>, CodecError> {
    let SecondaryLayoutContext {
        name,
        vertex_roots,
        vertex_live,
        vertex_ir,
        face_live,
        face_ir,
        half_edges,
        edge_by_half,
        grip_vertices,
        grip_points,
    } = *context;
    let live_vertices = vertex_ir.iter().flatten().count();
    let mut layouts = ctx.alloc_filled(live_vertices, None, "f3d subd secondary layouts")?;
    let mut has_cg = ctx.alloc_filled(
        vertex_live.len(),
        false,
        "f3d subd derived-grip ownership flags",
    )?;
    let mut secondary_counts =
        ctx.alloc_filled(vertex_live.len(), 0usize, "f3d subd secondary-grip counts")?;
    for marker in grip_vertices {
        if let GripVertexMarker::Secondary(Some(vertex)) = marker {
            *secondary_counts
                .get_mut(*vertex)
                .ok_or_else(|| malformed(name, "secondary grip vertex is out of range"))? += 1;
        }
    }

    for connectivity in derived_grips {
        let vertex = connectivity.vertex;
        if !vertex_live.get(vertex).copied().unwrap_or(false) {
            return Err(malformed(name, "derived-grip vertex is out of range"));
        }
        if has_cg[vertex] {
            return Err(malformed(
                name,
                "vertex has more than one derived-grip record",
            ));
        }
        has_cg[vertex] = true;
        let (root, direction) = vertex_roots
            .get(vertex)
            .copied()
            .flatten()
            .ok_or_else(|| malformed(name, "derived-grip vertex has no root direction"))?;
        let fan = build_fan(name, vertex, root, half_edges, face_live)?;
        if connectivity.spoke_lengths.len() != fan.len() {
            return Err(malformed(
                name,
                "derived-grip wedge count does not match the completed vertex fan",
            ));
        }

        let offset = direction_offset(direction);
        let mut cursor = 0usize;
        let mut wedges = Vec::new();
        for (wedge, &spoke_count) in connectivity.spoke_lengths.iter().enumerate() {
            let sector_count = spoke_count
                .checked_mul(
                    connectivity.spoke_lengths[(wedge + 1) % connectivity.spoke_lengths.len()],
                )
                .ok_or_else(|| malformed(name, "derived-grip sector arity overflows"))?;
            let slot = fan[(wedge + offset) % fan.len()];
            let FanSlot::Slot { half_edge, face } = slot else {
                if spoke_count != 0 {
                    return Err(malformed(
                        name,
                        "phantom wedge carries a nonzero spoke length",
                    ));
                }
                wedges.push(SubdGripWedge::Phantom {});
                continue;
            };
            let edge = Some(
                edge_by_half
                    .get(half_edge)
                    .copied()
                    .flatten()
                    .map(|(edge, _)| edge)
                    .ok_or_else(|| malformed(name, "fan half-edge has no owning edge"))?,
            );
            let sector_face = match face {
                Some(face) => Some(
                    face_ir
                        .get(face)
                        .copied()
                        .flatten()
                        .ok_or_else(|| malformed(name, "fan sector names a deleted face"))?,
                ),
                None => None,
            };
            let spokes = grip_block(
                name,
                grip_points,
                &connectivity.grip_indices,
                &mut cursor,
                spoke_count,
            )?;
            let sectors = grip_block(
                name,
                grip_points,
                &connectivity.grip_indices,
                &mut cursor,
                sector_count,
            )?;
            wedges.push(SubdGripWedge::Slot {
                edge,
                sector_face,
                spokes,
                sectors,
            });
        }
        if cursor != connectivity.grip_indices.len() {
            return Err(malformed(name, "derived-grip run has trailing entries"));
        }
        let vertex_ir =
            vertex_ir[vertex].ok_or_else(|| malformed(name, "derived-grip vertex is deleted"))?;
        layouts[index_from_u32(vertex_ir)] = Some(
            SubdVertexGripLayout::new(direction, wedges).map_err(|error| malformed(name, error))?,
        );
    }

    for (vertex, count) in secondary_counts.into_iter().enumerate() {
        if (count != 0) != has_cg[vertex] {
            return Err(malformed(
                name,
                "secondary-grip ownership does not have exactly one derived-grip record",
            ));
        }
    }
    Ok(layouts)
}

enum EditorSelectionKind {
    Edges,
    Vertices,
    Grips,
}

impl EditorSelectionKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Edges => "100edges",
            Self::Vertices => "100verts",
            Self::Grips => "50000grip",
        }
    }
}

fn parse(ctx: &DecodeContext<'_>, name: &str, bytes: &[u8]) -> Result<ParsedCage, CodecError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| malformed(name, format_args!("payload is not UTF-8: {error}")))?;
    if text.lines().next() != Some("#TS0200") {
        return Err(malformed(name, "unsupported header"));
    }

    // Every topology token occupies a slot in its own record order, and a bare
    // token is a deleted slot that occupies its index without defining an
    // element. Indices inside the program address slots, so slots are retained
    // through validation and compacted only when the IR cage is built.
    let mut face_roots: Vec<Option<usize>> = Vec::new();
    let mut edge_roots: Vec<Option<usize>> = Vec::new();
    let mut edge_knot_intervals: Vec<Option<PositiveReal>> = Vec::new();
    let mut edge_knot_records = Vec::new();
    let mut edge_knot_start = None;
    let mut edge_knot_finished = false;
    let mut edge_knot_contiguous = true;
    let mut vertex_roots: Vec<Option<(usize, SubdGripDirection)>> = Vec::new();
    let mut vertex_live: Vec<bool> = Vec::new();
    let mut half_edges: Vec<Option<ParsedHalfEdge>> = Vec::new();
    let mut crease_edges = BTreeSet::new();
    let mut grip_vertices: Vec<GripVertexMarker> = Vec::new();
    let mut grip_points: Vec<Option<GripPoint>> = Vec::new();
    let mut in_grip_map = false;
    let mut declarations = BTreeSet::new();
    let mut derived_grips = Vec::new();
    let mut selected_edges = BTreeSet::new();
    let mut selected_vertices = BTreeSet::new();
    let mut selected_grips = BTreeSet::new();
    let mut editor_declarations = BTreeSet::new();
    let mut symmetry_blocks = Vec::new();
    let mut current_symmetry: Option<PartialSymmetryBlock> = None;
    let mut terminal_declarations = BTreeSet::new();
    let mut unknown_record_kinds = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let mut fields = line.split_ascii_whitespace();
        let record_kind = fields.next();
        if record_kind == Some("106ek") {
            edge_knot_start.get_or_insert(edge_roots.len());
            edge_knot_contiguous &= !edge_knot_finished;
        } else if edge_knot_start.is_some() {
            edge_knot_finished = true;
        }
        let selection = match record_kind {
            Some("100edges") => Some(EditorSelectionKind::Edges),
            Some("100verts") => Some(EditorSelectionKind::Vertices),
            Some("50000grip") => Some(EditorSelectionKind::Grips),
            _ => None,
        };
        if let Some(selection) = selection {
            let label = selection.as_str();
            if !matches!(selection, EditorSelectionKind::Grips)
                && !editor_declarations.insert(label)
            {
                return Err(malformed(name, format_args!("duplicate {label} record")));
            }
            let mut values = BTreeSet::new();
            for value in fields {
                let index = parse_int::<usize>(name, Some(value), label)?;
                values.insert(index);
            }
            match selection {
                EditorSelectionKind::Edges => selected_edges = values,
                EditorSelectionKind::Vertices => selected_vertices = values,
                EditorSelectionKind::Grips => {
                    for value in values {
                        selected_grips.insert(value);
                    }
                }
            }
            continue;
        }
        match record_kind {
            Some("#TS0200") => require_end(name, fields, "header")?,
            Some("degree") => {
                if parse_int::<usize>(name, fields.next(), "degree")? != 3 {
                    return Err(malformed(name, "unsupported degree"));
                }
                require_end(name, fields, "degree declaration")?;
                declarations.insert("degree");
            }
            Some(declaration @ ("cap-type" | "end-conditions" | "star-knot-rule")) => {
                fields
                    .next()
                    .ok_or_else(|| malformed(name, format_args!("missing {declaration} value")))?;
                require_end(name, fields, declaration)?;
                declarations.insert(declaration);
            }
            Some("star-smoothness") => {
                parse_f64(name, fields.next(), "star smoothness")?;
                require_end(name, fields, "star-smoothness declaration")?;
                declarations.insert("star-smoothness");
            }
            Some("units") => {
                if fields.next() != Some("1") || fields.next() != Some("meters") {
                    return Err(malformed(name, "unsupported units declaration"));
                }
                require_end(name, fields, "units declaration")?;
                declarations.insert("units");
            }
            Some("f") => match fields.next() {
                None => face_roots.push(None),
                root => {
                    face_roots.push(Some(parse_int::<usize>(name, root, "face root")?));
                    parse_int::<i64>(name, fields.next(), "face flags")?;
                    require_end(name, fields, "face")?;
                }
            },
            Some("e") => match fields.next() {
                None => {
                    edge_roots.push(None);
                    edge_knot_intervals.push(None);
                }
                root => {
                    edge_roots.push(Some(parse_int::<usize>(name, root, "edge root")?));
                    let knot_interval = parse_f64(name, fields.next(), "edge knot interval")?;
                    let knot_interval = PositiveReal::new(knot_interval.get())
                        .ok_or_else(|| malformed(name, "edge knot interval is not positive"))?;
                    edge_knot_intervals.push(Some(knot_interval));
                    require_end(name, fields, "edge")?;
                }
            },
            Some("106ek") => {
                edge_knot_records.push(parse_f64(name, fields.next(), "106ek value")?);
                require_end(name, fields, "106ek")?;
            }
            Some("v") => match fields.next() {
                None => {
                    vertex_live.push(false);
                    vertex_roots.push(None);
                }
                root => {
                    let root = parse_int::<usize>(name, root, "vertex root")?;
                    let direction = parse_direction(name, fields.next())?;
                    require_end(name, fields, "vertex")?;
                    vertex_live.push(true);
                    vertex_roots.push(Some((root, direction)));
                }
            },
            Some("l") => match fields.next() {
                None => half_edges.push(None),
                next => {
                    let half = ParsedHalfEdge {
                        next: parse_int::<usize>(name, next, "half-edge next index")?,
                        previous: parse_int::<usize>(
                            name,
                            fields.next(),
                            "half-edge previous index",
                        )?,
                        mate: parse_int::<usize>(name, fields.next(), "half-edge mate index")?,
                        vertex: parse_int::<usize>(name, fields.next(), "half-edge vertex index")?,
                        face: parse_int::<i64>(name, fields.next(), "half-edge face index")?,
                    };
                    parse_int::<i64>(name, fields.next(), "half-edge edge index")?;
                    parse_int::<i64>(name, fields.next(), "half-edge flags")?;
                    if fields.next().is_some() {
                        return Err(malformed(name, "half-edge has trailing fields"));
                    }
                    half_edges.push(Some(half));
                }
            },
            Some("ec") => {
                crease_edges.insert(parse_int::<usize>(
                    name,
                    fields.next(),
                    "crease edge index",
                )?);
                parse_int::<i64>(name, fields.next(), "crease flags")?;
                require_end(name, fields, "crease")?;
            }
            Some("0m") => match fields.next() {
                Some("odd-grip-map") => {
                    require_end(name, fields, "odd-grip-map declaration")?;
                    in_grip_map = true;
                }
                Some("gvp") if in_grip_map => {
                    grip_vertices.push(GripVertexMarker::Primary(parse_int::<usize>(
                        name,
                        fields.next(),
                        "grip vertex index",
                    )?));
                    require_end(name, fields, "primary grip map")?;
                }
                Some("gv") if in_grip_map => {
                    let vertex =
                        parse_int::<i64>(name, fields.next(), "secondary grip vertex index")?;
                    if vertex < -1 {
                        return Err(malformed(name, "secondary grip vertex is below -1"));
                    }
                    grip_vertices.push(GripVertexMarker::Secondary(if vertex >= 0 {
                        Some(usize::try_from(vertex).map_err(|_| {
                            malformed(name, "secondary grip vertex exceeds address space")
                        })?)
                    } else {
                        None
                    }));
                    require_end(name, fields, "secondary grip map")?;
                }
                Some("cg") if in_grip_map => {
                    let vertex = parse_int::<usize>(name, fields.next(), "derived-grip vertex")?;
                    let wedges =
                        parse_int::<usize>(name, fields.next(), "derived-grip wedge count")?;
                    let mut spoke_lengths = Vec::new();
                    for _ in 0..wedges {
                        let length =
                            parse_int::<usize>(name, fields.next(), "derived-grip spoke length")?;
                        spoke_lengths.push(length);
                    }
                    if spoke_lengths.is_empty() {
                        return Err(malformed(name, "derived-grip wedge count is zero"));
                    }
                    let grip_count = spoke_lengths.iter().enumerate().try_fold(
                        0usize,
                        |count, (wedge, &spoke_count)| {
                            let cross = spoke_count
                                .checked_mul(spoke_lengths[(wedge + 1) % spoke_lengths.len()])
                                .ok_or_else(|| malformed(name, "derived-grip arity overflows"))?;
                            count
                                .checked_add(spoke_count)
                                .and_then(|count| count.checked_add(cross))
                                .ok_or_else(|| malformed(name, "derived-grip arity overflows"))
                        },
                    )?;
                    let mut grip_indices = Vec::new();
                    for _ in 0..grip_count {
                        let index =
                            match parse_int::<i64>(name, fields.next(), "derived-grip index")? {
                                -1 => None,
                                index => Some(usize::try_from(index).map_err(|_| {
                                    malformed(name, "derived-grip index is negative or overflows")
                                })?),
                            };
                        grip_indices.push(index);
                    }
                    require_end(name, fields, "derived-grip connectivity")?;
                    derived_grips.push(DerivedGripConnectivity {
                        vertex,
                        spoke_lengths,
                        grip_indices,
                    });
                }
                _ => return Err(malformed(name, "unknown odd-grip-map record")),
            },
            Some("0g") => match fields.next() {
                None => grip_points.push(None),
                x => {
                    let point = Point3::new(
                        parse_f64(name, x, "grip x")?.get() * CAGE_COORDINATE_SCALE,
                        parse_f64(name, fields.next(), "grip y")?.get() * CAGE_COORDINATE_SCALE,
                        parse_f64(name, fields.next(), "grip z")?.get() * CAGE_COORDINATE_SCALE,
                    );
                    let weight = parse_f64(name, fields.next(), "grip weight")?;
                    let weight = PositiveReal::new(weight.get())
                        .filter(|_| fields.next().is_none())
                        .ok_or_else(|| malformed(name, "grip weight is not positive"))?;
                    grip_points.push(Some(GripPoint { point, weight }));
                }
            },
            Some("105sym") => {
                let mode = match parse_int::<i64>(name, fields.next(), "symmetry flags")? {
                    0 => SymmetryMode::Correspondence,
                    1 => SymmetryMode::Radial,
                    _ => return Err(malformed(name, "unsupported symmetry flags")),
                };
                require_end(name, fields, "symmetry header")?;
                if let Some(block) = current_symmetry.replace(PartialSymmetryBlock::new(mode)) {
                    symmetry_blocks.push(block);
                }
            }
            Some("105plane") => {
                let block = current_symmetry
                    .as_mut()
                    .ok_or_else(|| malformed(name, "symmetry plane has no header"))?;
                let mut values = Vec::new();
                for value in fields {
                    values.push(parse_f64(name, Some(value), "symmetry plane coefficient")?);
                }
                let plane: [FiniteReal; 12] = values
                    .try_into()
                    .map_err(|_| malformed(name, "symmetry plane must have 12 coefficients"))?;
                if block.plane.replace(plane).is_some() {
                    return Err(malformed(name, "duplicate symmetry plane"));
                }
            }
            Some("105a") => {
                let kind = fields
                    .next()
                    .ok_or_else(|| malformed(name, "missing symmetry map kind"))?;
                let pairs = parse_pairs(name, fields, "symmetry map")?;
                let block = current_symmetry
                    .as_mut()
                    .ok_or_else(|| malformed(name, "symmetry map has no header"))?;
                if block.mode != SymmetryMode::Correspondence {
                    return Err(malformed(
                        name,
                        "correspondence map belongs to a radial symmetry block",
                    ));
                }
                if block.record_kinds.contains(kind) {
                    return Err(malformed(
                        name,
                        format_args!("duplicate {kind} symmetry map"),
                    ));
                }
                let kind_name = kind.to_owned();
                block.record_kinds.insert(kind_name);
                let target = match kind {
                    "fr" => &mut block.face_forward,
                    "f" => &mut block.face_reverse,
                    "er" => &mut block.edge_forward,
                    "e" => &mut block.edge_reverse,
                    "vr" => &mut block.vertex_forward,
                    "v" => &mut block.vertex_reverse,
                    _ => return Err(malformed(name, "unknown symmetry map kind")),
                };
                *target = pairs;
            }
            Some("105r") => {
                let kind = fields
                    .next()
                    .ok_or_else(|| malformed(name, "missing radial symmetry record kind"))?;
                let block = current_symmetry
                    .as_mut()
                    .ok_or_else(|| malformed(name, "radial symmetry record has no header"))?;
                if block.mode != SymmetryMode::Radial {
                    return Err(malformed(
                        name,
                        "radial symmetry record belongs to a correspondence block",
                    ));
                }
                if block.record_kinds.contains(kind) {
                    return Err(malformed(
                        name,
                        format_args!("duplicate {kind} radial symmetry record"),
                    ));
                }
                let kind_name = kind.to_owned();
                block.record_kinds.insert(kind_name);
                match kind {
                    "segments" => {
                        let segments =
                            parse_int::<usize>(name, fields.next(), "radial symmetry segments")?;
                        block.radial_segments = Some(
                            std::num::NonZeroU32::new(u32::try_from(segments).map_err(|_| {
                                malformed(name, "radial symmetry segments exceed u32")
                            })?)
                            .ok_or_else(|| {
                                malformed(name, "radial symmetry segments is not positive")
                            })?,
                        );
                        require_end(name, fields, "radial symmetry segments")?;
                    }
                    "sweep" => {
                        block.radial_sweep =
                            Some(parse_f64(name, fields.next(), "radial symmetry sweep")?);
                        require_end(name, fields, "radial symmetry sweep")?;
                    }
                    kind => {
                        let selector = match kind {
                            "ef" => SubdRadialMapSelector::Ef,
                            "er" => SubdRadialMapSelector::Er,
                            "ff" => SubdRadialMapSelector::Ff,
                            "fr" => SubdRadialMapSelector::Fr,
                            "vf" => SubdRadialMapSelector::Vf,
                            "vr" => SubdRadialMapSelector::Vr,
                            _ => {
                                return Err(malformed(
                                    name,
                                    format_args!("unknown radial symmetry record {kind}"),
                                ))
                            }
                        };
                        let pairs = parse_radial_pairs(name, fields)?;
                        block
                            .radial_maps
                            .push(SubdRadialSymmetryMap { selector, pairs });
                    }
                }
            }
            Some(declaration @ ("tol" | "geom-tol")) => {
                let tolerance = parse_f64(name, fields.next(), declaration)?;
                if tolerance.get() <= 0.0 {
                    return Err(malformed(
                        name,
                        format_args!("{declaration} is not positive"),
                    ));
                }
                require_end(name, fields, declaration)?;
                if !terminal_declarations.insert(declaration) {
                    return Err(malformed(name, format_args!("duplicate {declaration}")));
                }
            }
            Some(declaration @ ("ver" | "behavior-version" | "compat-version")) => {
                if fields.next().is_none() {
                    return Err(malformed(name, format_args!("missing {declaration} value")));
                }
                require_end(name, fields, declaration)?;
                if !terminal_declarations.insert(declaration) {
                    return Err(malformed(name, format_args!("duplicate {declaration}")));
                }
            }
            Some(kind) => {
                if let Some(count) = unknown_record_kinds.get_mut(kind) {
                    *count += 1;
                } else {
                    let key = kind.to_owned();
                    unknown_record_kinds.insert(key, 1);
                }
            }
            None => {}
        }
    }
    if let Some(block) = current_symmetry {
        symmetry_blocks.push(block);
    }

    let edge_user_knots_valid = !edge_knot_records.is_empty()
        && edge_knot_contiguous
        && edge_knot_start == Some(edge_roots.len())
        && edge_knot_records.len() == edge_roots.len()
        && edge_knot_records
            .iter()
            .zip(&edge_roots)
            .all(|(value, edge)| value.get() == -1.0 || edge.is_some() && value.get() >= 0.0);
    if !edge_knot_records.is_empty() && !edge_user_knots_valid {
        if let Some(count) = unknown_record_kinds.get_mut("106ek") {
            *count += edge_knot_records.len();
        } else {
            unknown_record_kinds.insert("106ek".to_owned(), edge_knot_records.len());
        }
    }

    let live_vertices = vertex_live.iter().filter(|live| **live).count();
    if declarations.len() != 6
        || !face_roots.iter().any(Option::is_some)
        || !edge_roots.iter().any(Option::is_some)
        || live_vertices == 0
        || !half_edges.iter().any(Option::is_some)
        || (!grip_vertices.is_empty() && grip_vertices.len() != grip_points.len())
    {
        return Err(malformed(name, "control cage is incomplete"));
    }
    let half_edges = compact_half_edges(
        ctx,
        name,
        &half_edges,
        &mut face_roots,
        &mut edge_roots,
        &mut vertex_roots,
    )?;

    let face_live = face_roots.iter().map(Option::is_some).collect::<Vec<_>>();
    let edge_live = edge_roots.iter().map(Option::is_some).collect::<Vec<_>>();
    for edge in &selected_edges {
        if !edge_live.get(*edge).copied().unwrap_or(false) {
            return Err(malformed(name, "selected edge is out of range or deleted"));
        }
    }
    for vertex in &selected_vertices {
        if !vertex_live.get(*vertex).copied().unwrap_or(false) {
            return Err(malformed(
                name,
                "selected vertex is out of range or deleted",
            ));
        }
    }
    for grip in &selected_grips {
        if !grip_points.get(*grip).is_some_and(Option::is_some) {
            return Err(malformed(name, "selected grip is out of range or deleted"));
        }
    }
    let mut typed_symmetry_blocks = Vec::new();
    for block in symmetry_blocks {
        let plane = block
            .plane
            .ok_or_else(|| malformed(name, "symmetry block has no plane"))?;
        let kind = match block.mode {
            SymmetryMode::Correspondence => {
                validate_symmetry_map(
                    name,
                    &block.face_forward,
                    &block.face_reverse,
                    &face_live,
                    "face",
                )?;
                validate_symmetry_map(
                    name,
                    &block.edge_forward,
                    &block.edge_reverse,
                    &edge_live,
                    "edge",
                )?;
                validate_symmetry_map(
                    name,
                    &block.vertex_forward,
                    &block.vertex_reverse,
                    &vertex_live,
                    "vertex",
                )?;
                SymmetryKind::Correspondence {
                    face: block.face_forward,
                    edge: block.edge_forward,
                    vertex: block.vertex_forward,
                }
            }
            SymmetryMode::Radial => {
                let segments = block
                    .radial_segments
                    .ok_or_else(|| malformed(name, "radial symmetry block is missing segments"))?;
                let sweep = block
                    .radial_sweep
                    .ok_or_else(|| malformed(name, "radial symmetry block is missing sweep"))?;
                for selector in [
                    SubdRadialMapSelector::Ef,
                    SubdRadialMapSelector::Er,
                    SubdRadialMapSelector::Ff,
                    SubdRadialMapSelector::Fr,
                    SubdRadialMapSelector::Vf,
                    SubdRadialMapSelector::Vr,
                ] {
                    if !block.radial_maps.iter().any(|map| map.selector == selector) {
                        return Err(malformed(
                            name,
                            format_args!(
                                "radial symmetry block is missing {}",
                                match selector {
                                    SubdRadialMapSelector::Ef => "ef",
                                    SubdRadialMapSelector::Er => "er",
                                    SubdRadialMapSelector::Ff => "ff",
                                    SubdRadialMapSelector::Fr => "fr",
                                    SubdRadialMapSelector::Vf => "vf",
                                    SubdRadialMapSelector::Vr => "vr",
                                }
                            ),
                        ));
                    }
                }
                SymmetryKind::Radial {
                    segments,
                    sweep,
                    maps: block.radial_maps,
                }
            }
        };
        typed_symmetry_blocks.push(SymmetryBlock { plane, kind });
    }
    let mut grip_owners =
        ctx.alloc_filled(grip_vertices.len(), None, "f3d subd secondary-grip owners")?;
    for connectivity in &derived_grips {
        if !vertex_live
            .get(connectivity.vertex)
            .copied()
            .unwrap_or(false)
        {
            return Err(malformed(name, "derived-grip connectivity is out of range"));
        }
        for &index in connectivity.grip_indices.iter().flatten() {
            if !matches!(grip_vertices.get(index), Some(GripVertexMarker::Secondary(Some(vertex))) if *vertex == connectivity.vertex)
            {
                return Err(malformed(
                    name,
                    "derived-grip entry is not a secondary grip of its vertex",
                ));
            }
            let owner_slot = grip_owners
                .get_mut(index)
                .ok_or_else(|| malformed(name, "derived-grip entry is out of range"))?;
            if owner_slot.replace(connectivity.vertex).is_some() {
                return Err(malformed(name, "secondary grip is named more than once"));
            }
        }
    }
    for (index, marker) in grip_vertices.iter().enumerate() {
        if let GripVertexMarker::Secondary(Some(vertex)) = marker {
            if grip_owners[index] != Some(*vertex) {
                return Err(malformed(
                    name,
                    "secondary grip is not named exactly once by derived connectivity",
                ));
            }
        }
    }
    for (marker, point) in grip_vertices.iter().zip(&grip_points) {
        match marker {
            GripVertexMarker::Primary(vertex) | GripVertexMarker::Secondary(Some(vertex)) => {
                if point.is_none() || !vertex_live.get(*vertex).copied().unwrap_or(false) {
                    return Err(malformed(name, "grip vertex map is inconsistent"));
                }
            }
            GripVertexMarker::Secondary(None) if point.is_some() => {
                return Err(malformed(name, "deleted grip marker has a point"));
            }
            GripVertexMarker::Secondary(None) => {}
        }
    }
    for (index, half) in half_edges.iter().enumerate() {
        let id = HalfEdgeId(index);
        let mate = &half_edges[half.mate.index()];
        let next = &half_edges[half.next.index()];
        let previous = &half_edges[half.previous.index()];
        if mate.mate != id
            || next.previous != id
            || previous.next != id
            || !vertex_live.get(half.vertex).copied().unwrap_or(false)
        {
            return Err(malformed(name, "half-edge topology is inconsistent"));
        }
    }
    for (vertex, live) in vertex_live.iter().copied().enumerate() {
        if live {
            let (root, _) = vertex_roots
                .get(vertex)
                .copied()
                .flatten()
                .ok_or_else(|| malformed(name, "live vertex has no root direction"))?;
            build_fan(name, vertex, root, &half_edges, &face_live)?;
        }
    }

    // Slot indices address the program; IR indices address only populated slots.
    let vertex_ir = compact(vertex_live.iter().copied())?;
    let edge_ir = compact(edge_roots.iter().map(Option::is_some))?;
    let face_ir = compact(face_roots.iter().map(Option::is_some))?;
    let mut symmetries = Vec::new();
    let mut unresolved_radial_pairs = 0;
    for block in typed_symmetry_blocks {
        let plane = symmetry_plane(name, block.plane)?;
        let (kind, face_pairs, edge_pairs, vertex_pairs) = match block.kind {
            SymmetryKind::Correspondence { face, edge, vertex } => (
                SubdSymmetryKind::Correspondence {},
                remap_symmetry_pairs(name, &face, &face_ir, "face")?,
                remap_symmetry_pairs(name, &edge, &edge_ir, "edge")?,
                remap_symmetry_pairs(name, &vertex, &vertex_ir, "vertex")?,
            ),
            SymmetryKind::Radial {
                segments,
                sweep,
                maps,
            } => {
                let projected = project_radial_maps(&maps, &face_ir, &edge_ir, &vertex_ir);
                unresolved_radial_pairs += projected.unresolved;
                (
                    SubdSymmetryKind::radial_from_parts(segments, sweep, maps)
                        .map_err(|error| malformed(name, error))?,
                    projected.face,
                    projected.edge,
                    projected.vertex,
                )
            }
        };
        let symmetry = SubdSymmetry::new(kind, plane, face_pairs, edge_pairs, vertex_pairs)
            .map_err(|error| malformed(name, error))?;
        symmetries.push(symmetry);
    }
    let edge_knot_intervals_ir = edge_knot_intervals
        .iter()
        .copied()
        .flatten()
        .collect::<Vec<_>>();
    let vertex_of = |slot: usize| {
        vertex_ir
            .get(slot)
            .copied()
            .flatten()
            .ok_or_else(|| malformed(name, "half-edge names a deleted vertex slot"))
    };

    let mut vertex_points = BTreeMap::new();
    if grip_vertices.is_empty() {
        if grip_points.len() != vertex_live.len() {
            return Err(malformed(name, "positional grip vertex map is incomplete"));
        }
        for (slot, point) in grip_points.iter().enumerate() {
            if let (true, Some(point)) = (vertex_live[slot], point) {
                let vertex = vertex_of(slot)?;
                vertex_points.insert(vertex, point.point);
            }
        }
    } else {
        for (marker, point) in grip_vertices.iter().zip(grip_points.iter()) {
            let (GripVertexMarker::Primary(slot), Some(point)) = (marker, point) else {
                continue;
            };
            let vertex = vertex_of(*slot)?;
            if vertex_points.contains_key(&vertex) {
                return Err(malformed(name, "primary grip vertex map is inconsistent"));
            }
            vertex_points.insert(vertex, point.point);
        }
    }
    if vertex_points.len() != live_vertices {
        return Err(malformed(name, "primary grip vertex map is incomplete"));
    }

    let mut edge_by_half =
        ctx.alloc_filled(half_edges.len(), None, "f3d subd half-edge ownership")?;
    let mut edge_vertices = Vec::new();
    for (edge, root) in (0_u32..).zip(edge_roots.iter().copied().flatten()) {
        let half = &half_edges[root];
        if edge_by_half[root].replace((edge, false)).is_some()
            || edge_by_half[half.mate.index()]
                .replace((edge, true))
                .is_some()
        {
            return Err(malformed(name, "edge roots reuse a half-edge"));
        }
        let mate = &half_edges[half.mate.index()];
        edge_vertices.push([vertex_of(mate.vertex)?, vertex_of(half.vertex)?]);
    }
    if edge_by_half.iter().any(Option::is_none) {
        return Err(malformed(name, "edge roots do not cover every half-edge"));
    }
    if edge_knot_intervals_ir.len() != edge_vertices.len() {
        return Err(malformed(name, "edge knot interval map is incomplete"));
    }

    let mut secondary_layouts = build_secondary_layouts(
        ctx,
        &SecondaryLayoutContext {
            name,
            vertex_roots: &vertex_roots,
            vertex_live: &vertex_live,
            vertex_ir: &vertex_ir,
            face_live: &face_live,
            face_ir: &face_ir,
            half_edges: &half_edges,
            edge_by_half: &edge_by_half,
            grip_vertices: &grip_vertices,
            grip_points: &grip_points,
        },
        &derived_grips,
    )?;

    let mut faces = Vec::new();
    for (face_slot, start) in face_roots.iter().copied().enumerate() {
        let Some(start) = start else { continue };
        let start_id = HalfEdgeId(start);
        let mut ring = Vec::new();
        let mut current = start_id;
        loop {
            let half = &half_edges[current.index()];
            if half.face != Some(face_slot) {
                return Err(malformed(name, "face ring carries a different face index"));
            }
            let (edge, reversed) = edge_by_half[current.index()]
                .ok_or_else(|| malformed(name, "face half-edge has no edge"))?;
            ring.push(SubdEdgeUse { edge, reversed });
            current = half.next;
            if current == start_id {
                break;
            }
            if ring.len() > half_edges.len() {
                return Err(malformed(name, "face ring does not close"));
            }
        }
        faces.push(SubdFace::new(ring).map_err(|error| malformed(name, error))?);
    }

    let mut crease_incidence =
        ctx.alloc_filled(live_vertices, 0usize, "f3d subd crease incidence")?;
    for edge in &crease_edges {
        let vertices = edge_ir
            .get(*edge)
            .copied()
            .flatten()
            .and_then(|edge| edge_vertices.get(index_from_u32(edge)))
            .ok_or_else(|| malformed(name, "crease edge is out of range"))?;
        crease_incidence[index_from_u32(vertices[0])] += 1;
        crease_incidence[index_from_u32(vertices[1])] += 1;
    }
    let mut vertices = Vec::new();
    for index in 0..live_vertices {
        let vertex_index = u32::try_from(index)
            .map_err(|_| malformed(name, "T-spline vertex index exceeds u32"))?;
        let vertex = SubdVertex::new(
            vertex_points[&vertex_index],
            match crease_incidence[index] {
                0 => SubdVertexTag::Smooth,
                1 => SubdVertexTag::Dart,
                2 => SubdVertexTag::Crease,
                _ => SubdVertexTag::Corner,
            },
            secondary_layouts[index].take(),
        )
        .map_err(|error| malformed(name, error))?;
        vertices.push(vertex);
    }
    let mut creased_edges = BTreeSet::new();
    for slot in &crease_edges {
        if let Some(edge) = edge_ir.get(*slot).copied().flatten() {
            creased_edges.insert(edge);
        }
    }
    let mut user_knots = ctx.alloc_filled(edge_vertices.len(), None, "map T-spline user knots")?;
    if edge_user_knots_valid {
        for (slot, value) in edge_knot_records.iter().enumerate() {
            let Some(edge) = edge_ir[slot] else {
                continue;
            };
            user_knots[index_from_u32(edge)] = Some(if value.get() == -1.0 {
                SubdUserKnot::Inherit
            } else {
                SubdUserKnot::Override(
                    NonNegativeReal::new(value.get())
                        .ok_or_else(|| malformed(name, "invalid user knot interval"))?,
                )
            });
        }
    }
    let mut edges = Vec::new();
    for (index, vertices) in edge_vertices.into_iter().enumerate() {
        let edge_index =
            u32::try_from(index).map_err(|_| malformed(name, "T-spline edge index exceeds u32"))?;
        let crease = creased_edges.contains(&edge_index);
        let sharpness = if crease { FULL_CREASE_SHARPNESS } else { 0.0 };
        let mut edge = SubdEdge::from_parts(
            vertices,
            [sharpness; 2],
            if crease {
                SubdEdgeTag::Crease
            } else {
                SubdEdgeTag::Smooth
            },
            Some(edge_knot_intervals_ir[index]),
            [0.0, 0.0],
        )
        .map_err(|error| malformed(name, error))?;
        edge.user_knot = user_knots[index];
        edges.push(edge);
    }
    let source_key = name
        .rsplit_once('/')
        .map_or(name, |(_, base)| base)
        .strip_suffix(".tsm")
        .unwrap_or(name);
    Ok(ParsedCage {
        surface: SubdSurface {
            id: subd_id(name, source_key)?,
            scheme: SubdScheme::CatmullClark,
            source_object: Some(SourceObjectAssociation {
                geometry_role: None,
                format: cadmpeg_ir::CodecFormat::F3d,
                object_id: cadmpeg_core::text::NonBlankString::new(name.to_owned())
                    .ok_or_else(|| malformed(name, "source object_id must not be empty"))?,
                name: None,
                color: None,
                visible: None,
                layer: None,
                instance_path: Vec::new(),
            }),
            cage: cadmpeg_ir::subd::SubdCage::new(vertices, edges, faces, symmetries)
                .map_err(|error| malformed(name, error))?,
        },
        unknown_record_kinds,
        unresolved_radial_pairs,
    })
}

#[cfg(test)]
mod tests {
    mod user_knots;

    use cadmpeg_ir::subd;
    use cadmpeg_test_support::wire;

    use super::SubdGripWedge;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    const EPS_KNOT_INTERVAL: f64 = 1.0e-12;

    const QUAD_TOPOLOGY: &str = "degree 3\n\
cap-type G1CAPS\n\
star-smoothness 0\n\
units 1 meters\n\
end-conditions SUBD_CREASES\n\
star-knot-rule NURCCS\n\
f 0 0\n\
e 0 1\ne 2 1\ne 4 1\ne 6 1\n\
v 0 NORTH\nv 2 NORTH\nv 4 NORTH\nv 6 NORTH\n\
l 2 6 1 0 0 0 0\nl 7 3 0 3 -1 0 0\n\
l 4 0 3 1 0 0 0\nl 1 5 2 0 -1 0 0\n\
l 6 2 5 2 0 0 0\nl 3 7 4 1 -1 0 0\n\
l 0 4 7 3 0 0 0\nl 5 1 6 2 -1 0 0\n\
ec 0 0\nec 1 0\nec 2 0\nec 3 0\n";

    fn parse_cage(bytes: &[u8]) -> Result<super::ParsedCage, cadmpeg_core::CodecError> {
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &DecodePolicy::default())
            .expect("test decode context");
        super::parse(&ctx, "synthetic.tsm", bytes)
    }

    #[test]
    fn tsm_source_identity_matches_composed_identity() {
        let actual = super::subd_id("synthetic.tsm", "synthetic").unwrap();
        assert_eq!(actual, crate::ids::subd_id("synthetic").unwrap());
    }

    #[cfg(target_pointer_width = "32")]
    fn derived_quad_source() -> String {
        format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gv 0\n\
             0m cg 0 4 1 0 0 0 4\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n0g 0.5 0 0 1\n"
        )
    }

    #[test]
    #[cfg(target_pointer_width = "32")]
    fn secondary_grip_vertex_refuses_32_bit_wrap() {
        let source = derived_quad_source();
        parse_cage(source.as_bytes()).expect("fixture derived grip is valid");
        let invalid = source.replace("0m gv 0\n", "0m gv 4294967296\n");
        let error =
            parse_cage(invalid.as_bytes()).expect_err("secondary grip index exceeds address space");
        assert!(error
            .to_string()
            .contains("secondary grip vertex exceeds address space"));
    }

    #[test]
    fn parses_explicit_grip_map() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    #[test]
    fn parses_positional_grip_map() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    #[test]
    fn counts_records_without_typed_semantics() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             vendor-extension 1 2 3\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_eq!(
            cage.unknown_record_kinds,
            std::collections::BTreeMap::from([("vendor-extension".to_string(), 1)])
        );
        assert_quad(&cage.surface);
    }

    #[test]
    fn parses_editor_metadata_and_derived_grip_connectivity() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gv 0\n\
             0m cg 0 4 1 0 0 0 4\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n0g 0.5 0 0 1\n\
             100edges 0 2\n100verts 1\n50000grip 0\n50000grip 1\n\
             105sym 0\n105plane 0 2 0 1 0 1 0 0 0 0 1 0\n\
             105a fr 0 0\n105a er 0 0 1 2\n105a e 2 1\n\
             105a vr 0 1\n105a v 1 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("typed metadata");
        assert!(cage.unknown_record_kinds.is_empty());
        assert_quad(&cage.surface);
        let layout = wire::field_or_default::<Option<subd::SubdVertexGripLayout>>(
            &(wire::field::<Vec<subd::SubdVertex>>(&(cage.surface.cage), "vertices")[0]),
            "secondary_grips",
        )
        .expect("secondary grip layout");
        assert_eq!(
            wire::field::<subd::SubdGripDirection>(&(layout), "direction"),
            cadmpeg_ir::SubdGripDirection::North
        );
        assert_eq!(
            wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges").len(),
            4
        );
        let SubdGripWedge::Slot { spokes, .. } =
            &wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges")[0]
        else {
            panic!("first wedge is a fan slot");
        };
        assert_eq!(
            wire::field::<u32>(&(spokes[0].as_ref().unwrap()), "source_index"),
            4
        );
        assert!(
            wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges")[2..]
                .iter()
                .all(|wedge| matches!(wedge, SubdGripWedge::Phantom {}))
        );
        assert!(matches!(
            &wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges")[1],
            SubdGripWedge::Slot {
                sector_face: None,
                ..
            }
        ));

        assert_eq!(
            wire::field_or_default::<Vec<subd::SubdSymmetry>>(&(cage.surface.cage), "symmetries")
                .len(),
            1
        );
        let symmetry =
            &wire::field_or_default::<Vec<subd::SubdSymmetry>>(&(cage.surface.cage), "symmetries")
                [0];
        assert_eq!(
            wire::field::<subd::SubdSymmetryKind>(&(symmetry), "kind"),
            cadmpeg_ir::SubdSymmetryKind::Correspondence {}
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Point3>(&(symmetry.plane), "origin"),
            cadmpeg_ir::math::Point3::new(0.0, 20.0, 0.0)
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Vector3>(&(symmetry.plane), "first_axis"),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Vector3>(&(symmetry.plane), "second_axis"),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0)
        );
        assert_eq!(
            wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "face_pairs"),
            vec![[0, 0]]
        );
        assert_eq!(
            wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "edge_pairs"),
            vec![[0, 0], [1, 2]]
        );
        assert_eq!(
            wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "vertex_pairs"),
            vec![[0, 1]]
        );
    }

    #[test]
    fn rejects_secondary_grips_on_phantom_wedges() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0m gv 0\n0m gv 0\n\
             0m cg 0 4 1 0 1 0 4 5\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             0g 0.5 0 0 1\n0g 0.6 0 0 1\n"
        );
        let error = parse_cage(source.as_bytes()).expect_err("phantom spoke");
        assert!(
            error
                .to_string()
                .contains("phantom wedge carries a nonzero spoke length"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn partitions_rectangular_sector_grids_with_product_arity() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n\
             0m gv 0\n0m gv 0\n0m gv 0\n0m gv 0\n0m gv 0\n\
             0m cg 0 4 2 1 0 0 4 5 6 7 8\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             0g 0.1 0 0 1\n0g 0.2 0 0 1\n0g 0.3 0 0 1\n0g 0.4 0 0 1\n0g 0.5 0 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("rectangular sector grid");
        let layout = wire::field_or_default::<Option<subd::SubdVertexGripLayout>>(
            &(wire::field::<Vec<subd::SubdVertex>>(&(cage.surface.cage), "vertices")[0]),
            "secondary_grips",
        )
        .expect("secondary grip layout");
        let SubdGripWedge::Slot {
            spokes: first_spokes,
            sectors: first_sectors,
            ..
        } = &wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges")[0]
        else {
            panic!("first wedge is a fan slot");
        };
        let SubdGripWedge::Slot {
            spokes: second_spokes,
            sectors: second_sectors,
            ..
        } = &wire::field::<Vec<subd::SubdGripWedge>>(&(layout), "wedges")[1]
        else {
            panic!("second wedge is a fan slot");
        };
        assert_eq!(first_spokes.len(), 2);
        assert_eq!(first_sectors.len(), 2);
        assert_eq!(second_spokes.len(), 1);
        assert!(second_sectors.is_empty());
        assert_eq!(
            first_spokes
                .iter()
                .chain(first_sectors)
                .chain(second_spokes)
                .map(|grip| wire::field::<u32>(&(grip.as_ref().unwrap()), "source_index"))
                .collect::<Vec<_>>(),
            vec![4, 5, 6, 7, 8]
        );
    }

    #[test]
    fn maps_compass_words_to_north_anchored_offsets() {
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::North),
            0
        );
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::East),
            1
        );
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::South),
            2
        );
        assert_eq!(
            super::direction_offset(cadmpeg_ir::SubdGripDirection::West),
            3
        );
    }

    #[test]
    fn transfers_knot_intervals_and_absolute_crease_sharpness() {
        let source = QUAD_TOPOLOGY
            .replace("e 0 1\n", "e 0 0.5\n")
            .replace("e 2 1\n", "e 2 0.25\n")
            .replace("e 4 1\n", "e 4 0.125\n")
            .replace("e 6 1\n", "e 6 0.0625\n");
        let cage = parse_cage(
            format!(
                "#TS0200\n{source}\
                 0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
            )
            .as_bytes(),
        )
        .expect("knot intervals");
        let expected = [0.5, 0.25, 0.125, 0.0625];
        for (edge, expected) in wire::field::<Vec<subd::SubdEdge>>(&(cage.surface.cage), "edges")
            .iter()
            .zip(expected)
        {
            let actual = wire::field_or_default::<Option<f64>>(&(edge), "knot_interval")
                .expect("knot interval");
            assert!((actual - expected).abs() < EPS_KNOT_INTERVAL);
            assert!(wire::field::<[f64; 2]>(&(edge), "sharpness")
                .iter()
                .all(|sharpness| (*sharpness - 1.0).abs() < EPS_KNOT_INTERVAL));
        }
    }

    #[test]
    fn validates_the_subd_creases_edge_knot_mirror() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}106ek 0.9999999999999\n106ek 1\n106ek 1\n106ek 1\n\
                     0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("edge-knot mirror");
        assert!(cage.unknown_record_kinds.is_empty());
        assert_eq!(
            wire::field::<Vec<subd::SubdEdge>>(&(cage.surface.cage), "edges").len(),
            4
        );
    }

    #[test]
    fn validates_deleted_slots_in_the_subd_creases_edge_knot_mirror() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}e\n106ek 1\n106ek 1\n106ek 1\n106ek 1\n106ek -1\n\
                     0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("deleted edge-knot slot");
        assert!(cage.unknown_record_kinds.is_empty());
        assert_eq!(
            wire::field::<Vec<subd::SubdEdge>>(&(cage.surface.cage), "edges").len(),
            4
        );
    }

    #[test]
    fn transfers_explicit_zero_user_knot_overrides() {
        let source = format!(
            "#TS0200\n{}106ek 0\n106ek 1\n106ek 0\n106ek 1\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n",
            QUAD_TOPOLOGY.replace(
                "end-conditions SUBD_CREASES",
                "end-conditions MULTIPLE_KNOTS"
            )
        );
        let cage = parse_cage(source.as_bytes()).expect("user-knot overrides");
        assert!(cage.unknown_record_kinds.is_empty());
        let edges = cage.surface.cage.edges();
        for (edge, expected) in edges.iter().zip([0.0, 1.0, 0.0, 1.0]) {
            assert_eq!(edge.effective_knot_interval(), Some(expected));
            assert!(matches!(
                edge.user_knot,
                Some(subd::SubdUserKnot::Override(_))
            ));
            assert_eq!(
                wire::field_or_default::<Option<f64>>(edge, "knot_interval"),
                Some(1.0)
            );
        }
    }

    #[test]
    fn parses_radial_symmetry_metadata() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             105sym 1\n105plane 0.5 0.5 0 1 1 0 0 0 0 1 0 0\n\
             105r segments 4\n105r sweep 1\n\
             105r ef 0 1\n105r er 1 0\n105r ff 0 0\n105r fr 0 0\n\
             105r vf 0 1\n105r vr 1 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("radial symmetry metadata");
        assert!(cage.unknown_record_kinds.is_empty());
        assert_quad(&cage.surface);
        assert_eq!(
            wire::field_or_default::<Vec<subd::SubdSymmetry>>(&(cage.surface.cage), "symmetries")
                .len(),
            1
        );
        let symmetry =
            &wire::field_or_default::<Vec<subd::SubdSymmetry>>(&(cage.surface.cage), "symmetries")
                [0];
        let cadmpeg_ir::SubdSymmetryKind::Radial(radial) =
            wire::field::<subd::SubdSymmetryKind>(&(symmetry), "kind")
        else {
            panic!("radial symmetry kind");
        };
        let radial_maps =
            wire::field_or_default::<Vec<subd::SubdRadialSymmetryMap>>(&(radial), "radial_maps");
        assert_eq!(
            (
                wire::field::<std::num::NonZeroU32>(&(radial), "segments").get(),
                wire::field::<f64>(&(radial), "sweep")
            ),
            (4, 1.0)
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Point3>(&(symmetry.plane), "origin"),
            cadmpeg_ir::math::Point3::new(5.0, 5.0, 0.0)
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Vector3>(&(symmetry.plane), "first_axis"),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
        );
        assert_eq!(
            wire::field::<cadmpeg_ir::math::Vector3>(&(symmetry.plane), "second_axis"),
            cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0)
        );
        assert_eq!(
            *wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "face_pairs"),
            vec![[0, 0]]
        );
        assert_eq!(
            *wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "edge_pairs"),
            vec![[0, 1]]
        );
        assert_eq!(
            *wire::field_or_default::<Vec<[u32; 2]>>(&(symmetry), "vertex_pairs"),
            vec![[0, 1]]
        );
        assert_eq!(
            *radial_maps,
            vec![
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Ef,
                    pairs: vec![[0, 1]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Er,
                    pairs: vec![[1, 0]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Ff,
                    pairs: vec![[0, 0]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Fr,
                    pairs: vec![[0, 0]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Vf,
                    pairs: vec![[0, 1]],
                },
                cadmpeg_ir::SubdRadialSymmetryMap {
                    selector: cadmpeg_ir::SubdRadialMapSelector::Vr,
                    pairs: vec![[1, 0]],
                },
            ]
        );

        let unsupported = source.replace("105sym 1", "105sym 2");
        let error = parse_cage(unsupported.as_bytes()).expect_err("unsupported symmetry mode");
        assert!(
            error.to_string().contains("unsupported symmetry flags"),
            "unexpected error: {error}"
        );

        let missing = source.replace("105r vf 0 1\n", "");
        let error = parse_cage(missing.as_bytes()).expect_err("incomplete radial maps");
        assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));

        let native_id = u64::MAX;
        let replacement = format!("105r ef {native_id} {native_id}\n");
        let native = source.replace("105r ef 0 1\n", &replacement);
        let cage = parse_cage(native.as_bytes()).expect("opaque radial native id");
        let cadmpeg_ir::SubdSymmetryKind::Radial(radial) = wire::field::<subd::SubdSymmetryKind>(
            &(wire::field_or_default::<Vec<subd::SubdSymmetry>>(
                &(cage.surface.cage),
                "symmetries",
            )[0]),
            "kind",
        ) else {
            panic!("radial symmetry kind");
        };
        let ef =
            wire::field_or_default::<Vec<subd::SubdRadialSymmetryMap>>(&(radial), "radial_maps")
                .into_iter()
                .find(|map| map.selector == cadmpeg_ir::SubdRadialMapSelector::Ef)
                .expect("ef radial map");
        assert_eq!(ef.pairs, vec![[native_id, native_id]]);
    }

    #[test]
    fn radial_indices_abstain_when_deleted_slot_models_disagree() {
        let slots = [Some(0), None, Some(1), Some(2)];
        assert_eq!(super::agreed_radial_index(0, &slots, 3), Some(0));
        for id in [1, 2, 3, 4, u64::MAX] {
            assert_eq!(super::agreed_radial_index(id, &slots, 3), None);
        }
        let dense = [Some(0), Some(1), Some(2)];
        assert_eq!(super::agreed_radial_index(2, &dense, 3), Some(2));
        assert_eq!(super::agreed_radial_index(3, &dense, 3), None);
    }

    #[test]
    fn radial_domains_conflicts_and_inverse_pairs_do_not_get_fallbacks() {
        let dense = [Some(0), Some(1), Some(2), Some(3)];
        let (pairs, unresolved) = super::project_radial_pairs(&[[0, 1], [2, 1]], &dense);
        assert!(pairs.is_empty());
        assert_eq!(unresolved, 2);
        let maps = [subd::SubdRadialSymmetryMap {
            selector: subd::SubdRadialMapSelector::Ff,
            pairs: vec![[3, 0]],
        }];
        let projected = super::project_radial_maps(&maps, &[Some(0)], &dense, &dense);
        assert!(projected.face.is_empty());
        assert!(projected.edge.is_empty());
        assert!(projected.vertex.is_empty());
        assert_eq!(projected.unresolved, 1);
        let mut unmatched = 0;
        assert!(super::mutual_radial_pairs(vec![[0, 1]], &[[0, 1]], &mut unmatched).is_empty());
        assert_eq!(unmatched, 2);
    }

    #[derive(serde::Serialize, serde::Deserialize)]
    struct RadialFixture {
        source: String,
        face_pairs: Vec<[u32; 2]>,
        edge_pairs: Vec<[u32; 2]>,
        vertex_pairs: Vec<[u32; 2]>,
        unresolved_pairs: usize,
        raw_maps: Vec<subd::SubdRadialSymmetryMap>,
    }

    #[test]
    fn radial_source_maps_keep_raw_pairs_and_resolve_proved_domains() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             105sym 1\n105plane 0.5 0.5 0 1 1 0 0 0 0 1 0 0\n\
             105r segments 4\n105r sweep 1\n\
             105r ef 0 1\n105r er 1 0\n105r ff 0 0\n105r fr 0 0\n\
             105r vf 0 1\n105r vr 1 0\n"
        );
        let raw_maps = [
            (subd::SubdRadialMapSelector::Ef, [0, 1]),
            (subd::SubdRadialMapSelector::Er, [1, 0]),
            (subd::SubdRadialMapSelector::Ff, [0, 0]),
            (subd::SubdRadialMapSelector::Fr, [0, 0]),
            (subd::SubdRadialMapSelector::Vf, [0, 1]),
            (subd::SubdRadialMapSelector::Vr, [1, 0]),
        ]
        .into_iter()
        .map(|(selector, pair)| subd::SubdRadialSymmetryMap {
            selector,
            pairs: vec![pair],
        })
        .collect();
        let synthetic = serde_json::to_string(&[RadialFixture {
            source,
            face_pairs: vec![[0, 0]],
            edge_pairs: vec![[0, 1]],
            vertex_pairs: vec![[0, 1]],
            unresolved_pairs: 0,
            raw_maps,
        }])
        .unwrap();
        let records = crate::test_support::verification::text("tsm/radial.json", &synthetic);
        let cases: Vec<RadialFixture> = serde_json::from_str(&records).unwrap();
        assert_eq!(
            cases.len(),
            crate::test_support::verification::case_count(1, 29)
        );
        let mut unresolved = 0;
        for case in cases {
            let parsed = parse_cage(case.source.as_bytes()).expect("radial source cage");
            assert_eq!(parsed.unresolved_radial_pairs, case.unresolved_pairs);
            unresolved += parsed.unresolved_radial_pairs;
            let symmetries = wire::field_or_default::<Vec<subd::SubdSymmetry>>(
                &parsed.surface.cage,
                "symmetries",
            );
            let radial: Vec<_> = symmetries
                .iter()
                .filter(|symmetry| {
                    matches!(
                        wire::field::<subd::SubdSymmetryKind>(symmetry, "kind"),
                        subd::SubdSymmetryKind::Radial(_)
                    )
                })
                .collect();
            assert_eq!(radial.len(), 1);
            let symmetry = radial[0];
            assert_eq!(
                *wire::field_or_default::<Vec<[u32; 2]>>(symmetry, "face_pairs"),
                case.face_pairs
            );
            assert_eq!(
                *wire::field_or_default::<Vec<[u32; 2]>>(symmetry, "edge_pairs"),
                case.edge_pairs
            );
            assert_eq!(
                *wire::field_or_default::<Vec<[u32; 2]>>(symmetry, "vertex_pairs"),
                case.vertex_pairs
            );
            let subd::SubdSymmetryKind::Radial(metadata) =
                wire::field::<subd::SubdSymmetryKind>(symmetry, "kind")
            else {
                panic!("radial metadata");
            };
            assert_eq!(
                *wire::field_or_default::<Vec<subd::SubdRadialSymmetryMap>>(
                    &metadata,
                    "radial_maps"
                ),
                case.raw_maps
            );
        }
        assert_eq!(
            unresolved,
            crate::test_support::verification::case_count(0, 52)
        );
    }

    #[test]
    fn rejects_nonorthonormal_symmetry_plane() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n\
             105sym 0\n105plane 0 0 0 1 1 0 0 0 1 0 0 0\n\
             tol 0.00001\nver 6021\nbehavior-version 6.5.0\n"
        );
        let error = parse_cage(source.as_bytes()).expect_err("nonorthonormal symmetry plane");
        assert!(
            error
                .to_string()
                .contains("symmetry plane is not a homogeneous orthonormal frame"),
            "unexpected error: {error}"
        );
    }

    /// A bare topology token is a deleted slot: it consumes an index and
    /// defines no element. Appending one of each leaves the cage unchanged.
    #[test]
    fn deleted_slots_consume_an_index_without_defining_an_element() {
        let source = format!(
            "#TS0200\n{QUAD_TOPOLOGY}f\ne\nv\nl\n\
             0m odd-grip-map\n0m gvp 0\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gv -1\n\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n0g\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("quad cage");
        assert_quad(&cage.surface);
    }

    /// Deleted slots renumber the IR: a leading deleted vertex and edge slot
    /// shift every program index by one without changing the emitted cage.
    #[test]
    fn deleted_slots_renumber_the_cage() {
        let shifted = QUAD_TOPOLOGY
            .replace("e 0 1\n", "e\ne 0 1\n")
            .replace("v 0 NORTH\n", "v\nv 0 NORTH\n")
            .replace(
                "ec 0 0\nec 1 0\nec 2 0\nec 3 0\n",
                "ec 1 0\nec 2 0\nec 3 0\nec 4 0\n",
            );
        let shifted = shifted.replace("l 2 6 1 0 0 0 0", "l 2 6 1 1 0 0 0");
        let shifted = shifted.replace("l 7 3 0 3 -1 0 0", "l 7 3 0 4 -1 0 0");
        let shifted = shifted.replace("l 4 0 3 1 0 0 0", "l 4 0 3 2 0 0 0");
        let shifted = shifted.replace("l 1 5 2 0 -1 0 0", "l 1 5 2 1 -1 0 0");
        let shifted = shifted.replace("l 6 2 5 2 0 0 0", "l 6 2 5 3 0 0 0");
        let shifted = shifted.replace("l 3 7 4 1 -1 0 0", "l 3 7 4 2 -1 0 0");
        let shifted = shifted.replace("l 0 4 7 3 0 0 0", "l 0 4 7 4 0 0 0");
        let shifted = shifted.replace("l 5 1 6 2 -1 0 0", "l 5 1 6 3 -1 0 0");
        let source = format!(
            "#TS0200\n{shifted}\
             0m odd-grip-map\n0m gv -1\n0m gvp 1\n0m gvp 2\n0m gvp 3\n0m gvp 4\n\
             0g\n0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n"
        );
        let cage = parse_cage(source.as_bytes()).expect("shifted quad cage");
        assert_quad(&cage.surface);
    }

    /// A populated half-edge may not name a deleted slot.
    #[test]
    fn a_half_edge_naming_a_deleted_slot_is_rejected() {
        let source = format!(
            "#TS0200\n{}\
             0g 0 0 0 1\n0g 1 0 0 1\n0g 1 1 0 1\n0g 0 1 0 1\n",
            QUAD_TOPOLOGY.replace("l 5 1 6 2 -1 0 0", "l")
        );
        let error = parse_cage(source.as_bytes()).expect_err("deleted mate");
        assert!(
            error.to_string().contains("names a deleted slot"),
            "unexpected error: {error}"
        );
    }

    fn assert_quad(cage: &subd::SubdSurface) {
        assert_eq!(
            wire::field::<Vec<subd::SubdVertex>>(&(cage.cage), "vertices").len(),
            4
        );
        assert_eq!(
            wire::field::<Vec<subd::SubdEdge>>(&(cage.cage), "edges").len(),
            4
        );
        assert_eq!(
            wire::field::<Vec<subd::SubdFace>>(&(cage.cage), "faces").len(),
            1
        );
        assert_eq!(
            wire::field::<Vec<subd::SubdVertex>>(&(cage.cage), "vertices")[1]
                .point()
                .x,
            10.0
        );
        assert!(wire::field::<Vec<subd::SubdEdgeUse>>(
            &(wire::field::<Vec<subd::SubdFace>>(&(cage.cage), "faces")[0]),
            "edges"
        )
        .iter()
        .all(|use_| !use_.reversed));
    }
}
