//! Recover a single cached imported part that cadmpeg 0.6.1 leaves outside its active partition.
//!
//! Uses only CRC-checked, budgeted bytes retained by cadmpeg. The source file stays untouched.
//! A small in-memory envelope gives the unchanged Parasolid geometry a partition description
//! understood by that decoder. Configuration, feature identity and suppression must agree;
//! edited parts and ambiguous/multiple cached bodies are refused rather than guessed.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use cadmpeg_ir::features::FeatureOperation;
use cadmpeg_ir::{CadIr, SourceFidelity};

#[path = "sldprt/analytic.rs"]
pub(crate) mod analytic;
#[path = "sldprt/colors.rs"]
mod colors;
pub(super) use colors::restore_colors;

const MAX_BODY_BYTES: usize = 64 << 20;
const WRAPPER: [u8; 16] = [0x23, 0x1d, 0xd5, 0x71, 0xda, 0x81, 0x48, 0xa2, 0xa8, 0x58, 0x98, 0xb2, 0x1b, 0x89, 0xef, 0x99];

pub(super) struct CachedPart {
    pub input: Vec<u8>,
    pub geometry: Vec<u8>,
}

pub(super) fn cached_part(ir: &CadIr, fidelity: &SourceFidelity) -> Result<Option<CachedPart>, String> {
    if !ir.model.bodies.is_empty() || !ir.model.faces.is_empty() || !ir.model.points.is_empty() {
        return Ok(None);
    }
    let [config] = ir.model.configurations.as_slice() else { return Ok(None) };
    if !config.active || ir.model.features.len() > 256 {
        return Ok(None);
    }
    let Some(index) = config.source_index else { return Ok(None) };
    let Some(records) = ir.native.namespace("sldprt").and_then(|n| n.arenas().get("features")) else { return Ok(None) };
    let classes: BTreeMap<_, _> = records
        .iter()
        .map(|r| {
            let fields = r.fields().map_err(|e| format!("SLDPRT: cached feature metadata: {e}"))?;
            Ok((r.id().to_owned(), fields.get("input_class").and_then(|v| v.as_str()).map(str::to_owned)))
        })
        .collect::<Result<_, String>>()?;
    let mut imported = None;
    for feature in &ir.model.features {
        let active = config
            .feature_states
            .get(&feature.id)
            .map(|s| !s.evaluation.is_suppressed())
            .or_else(|| feature.suppressed.map(|suppressed| !suppressed))
            .ok_or("SLDPRT: cached recovery requires known feature suppression states")?;
        if !active {
            continue;
        }
        match feature.evaluation.definition().operation() {
            FeatureOperation::TreeNode { .. } | FeatureOperation::DatumPrincipalPlane { .. } => {}
            FeatureOperation::Native { kind, .. } if kind.as_str() == "Multi_CAD_Imported" => {
                if imported.is_some() {
                    return Ok(None);
                }
                imported = feature.name.as_deref();
            }
            FeatureOperation::Native { parameters, .. } => {
                let class = feature.native_ref.as_deref().and_then(|id| classes.get(id)).and_then(Option::as_deref);
                let metadata_only =
                    class.is_none() && parameters.is_empty() && feature.dependencies.is_empty() && feature.evaluation.outputs().is_empty();
                if !metadata_only
                    && !class
                        .is_some_and(|c| c.ends_with("Folder_c") || matches!(c, "moDetailCabinet_c" | "moRefPlane_c" | "moOriginProfileFeature_c"))
                {
                    return Err(format!("SLDPRT: cached-part recovery does not support feature {:?} with class {class:?}", feature.name));
                }
            }
            _ => return Err(format!("SLDPRT: cached-part recovery does not support feature {:?}", feature.evaluation.definition().operation())),
        }
    }
    let Some(imported) = imported else { return Ok(None) };
    let stream = format!("Config-{index}-FeatureBodies/LocalBodies");
    let mut candidates = fidelity.retained_records().values().filter(|r| r.stream().eq_ignore_ascii_case(&stream));
    let Some(record) = candidates.next() else { return Ok(None) };
    if candidates.next().is_some() {
        return Err("SLDPRT: ambiguous cached part records".into());
    }
    let bytes = record.data().ok_or("SLDPRT: cached part bytes were not retained")?;
    if bytes.len() > MAX_BODY_BYTES || read_u32(bytes, 0)? != 1 {
        return Err("SLDPRT: only one bounded cached imported part is supported".into());
    }
    if bytes.get(4..7) != Some(&[0xff, 0xfe, 0xff]) {
        return Err("SLDPRT: unsupported cached part name encoding".into());
    }
    let count = usize::from(*bytes.get(7).ok_or("SLDPRT: truncated cached part name")?);
    if count == 0 || count > 128 {
        return Err("SLDPRT: invalid cached part name length".into());
    }
    let end = 8 + count * 2;
    let text = bytes.get(8..end).ok_or("SLDPRT: truncated cached part name")?;
    let mut chars = Vec::with_capacity(count);
    for [a, b] in text.as_chunks::<2>().0 {
        chars.push(u16::from_le_bytes([*a, *b]));
    }
    let name = String::from_utf16(&chars).map_err(|_| "SLDPRT: invalid cached part name")?;
    if name != imported {
        return Err("SLDPRT: cached part does not match the active imported feature".into());
    }
    let mut wrappers = bytes.windows(WRAPPER.len()).enumerate().filter(|(_, b)| *b == WRAPPER);
    let Some((at, _)) = wrappers.next() else { return Err("SLDPRT: cached part has no geometry wrapper".into()) };
    if wrappers.next().is_some() || at != end + 22 {
        return Err("SLDPRT: unsupported or ambiguous cached part framing".into());
    }
    let size = usize::try_from(read_u32(bytes, at + 16)?).map_err(|_| "SLDPRT: cached part is too large")?;
    let compressed = usize::try_from(read_u32(bytes, at + 20)?).map_err(|_| "SLDPRT: cached part is too large")?;
    if size == 0 || size > MAX_BODY_BYTES || compressed > MAX_BODY_BYTES {
        return Err("SLDPRT: cached geometry exceeds the import limit".into());
    }
    let member = bytes.get(at + 24..at + 24 + compressed).ok_or("SLDPRT: truncated cached geometry")?;
    if bytes.get(at + 24 + compressed..).is_none_or(|tail| tail.len() != 8)
        || usize::try_from(read_u32(bytes, at - 4)?).ok() != Some(bytes.len() - at)
    {
        return Err("SLDPRT: invalid cached geometry extent".into());
    }
    let mut decoder = flate2::bufread::ZlibDecoder::new(member);
    let mut geometry = Vec::new();
    decoder.by_ref().take((MAX_BODY_BYTES + 1) as u64).read_to_end(&mut geometry).map_err(|e| format!("SLDPRT: cached geometry: {e}"))?;
    if geometry.len() != size || decoder.total_in() != compressed as u64 {
        return Err("SLDPRT: cached geometry size mismatch".into());
    }
    Ok(Some(CachedPart { input: partition_envelope(&geometry)?, geometry }))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, String> {
    let word: [u8; 4] = bytes.get(at..at + 4).ok_or("SLDPRT: truncated cached body")?.try_into().map_err(|_| "SLDPRT: truncated cached body")?;
    Ok(u32::from_le_bytes(word))
}

fn partition_envelope(geometry: &[u8]) -> Result<Vec<u8>, String> {
    if geometry.get(..4) != Some(b"PS\0\0") {
        return Err("SLDPRT: cached geometry is not a Parasolid stream".into());
    }
    let length: [u8; 2] =
        geometry.get(4..6).ok_or("SLDPRT: truncated Parasolid header")?.try_into().map_err(|_| "SLDPRT: truncated Parasolid header")?;
    let end = 6 + usize::from(u16::from_be_bytes(length));
    let description = geometry.get(6..end).ok_or("SLDPRT: truncated Parasolid description")?;
    if !description.starts_with(b": TRANSMIT FILE created by modeller version ") {
        return Err("SLDPRT: unsupported cached Parasolid header".into());
    }
    let description = b"partition cached imported part";
    let mut payload = Vec::with_capacity(geometry.len() + description.len());
    payload.extend_from_slice(b"PS\0\0");
    payload.extend_from_slice(&u16::try_from(description.len()).map_err(|_| "SLDPRT: invalid description")?.to_be_bytes());
    payload.extend_from_slice(description);
    payload.extend_from_slice(geometry.get(end..).ok_or("SLDPRT: truncated geometry")?);
    let mut deflate = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    deflate.write_all(&payload).map_err(|e| e.to_string())?;
    let compressed = deflate.finish().map_err(|e| e.to_string())?;
    let name = b"Contents/Config-0-Partition";
    let mut out = Vec::with_capacity(compressed.len() + name.len() + 34);
    out.extend_from_slice(&[0x9d, 0x51, 0x9b, 0xc0, 0, 0, 0, 4]);
    out.extend_from_slice(&[0x14, 0, 6, 0, 8, 0]);
    for value in [
        0,
        crc32fast::hash(&payload),
        u32::try_from(compressed.len()).map_err(|_| "SLDPRT: geometry is too large")?,
        u32::try_from(payload.len()).map_err(|_| "SLDPRT: geometry is too large")?,
        u32::try_from(name.len()).map_err(|_| "SLDPRT: invalid section name")?,
    ] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend(name.iter().map(|b| b.rotate_left(4)));
    out.extend_from_slice(&compressed);
    Ok(out)
}

pub(super) fn reference_points(ir: &CadIr) -> Result<Vec<[f64; 3]>, String> {
    let count =
        ir.model.tessellations.iter().try_fold(0usize, |n, t| n.checked_add(t.vertex_count())).ok_or("SLDPRT: display geometry is too large")?;
    if count == 0 || count > 100_000 {
        return Err("SLDPRT: cached-part recovery needs bounded display geometry".into());
    }
    Ok(ir
        .model
        .tessellations
        .iter()
        .flat_map(|t| t.vertices())
        .map(|p| {
            let p = p.get();
            [p.x, p.y, p.z]
        })
        .collect())
}

/// Resolve only analytic boundaries supported by the original display geometry. No shortest-arc
/// heuristic: either branch must have exclusive interior evidence, otherwise import is refused.
pub(super) fn normalize_cached(ir: &mut CadIr, reference: &[[f64; 3]]) -> Result<(), String> {
    if ir
        .model
        .surfaces
        .iter()
        .any(|s| matches!(s.geometry, cadmpeg_ir::geometry::SurfaceGeometry::Solved(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Cone(_))))
        || ir.model.edges.iter().any(|e| e.start == e.end)
    {
        analytic::normalize(ir)?;
        return remove_empty_regions(ir);
    }
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::topology::{FaceLoops, Sense};
    use std::collections::BTreeSet;
    if ir.model.faces.len() > 512
        || ir.model.edges.len() > 4096
        || [ir.model.points.len(), ir.model.vertices.len(), ir.model.coedges.len(), ir.model.loops.len()].into_iter().any(|n| n > 8192)
    {
        return Err("SLDPRT: cached part exceeds the analytic recovery scope".into());
    }
    if ir
        .model
        .surfaces
        .iter()
        .any(|s| !matches!(s.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_) | SolvedSurfaceGeometry::Cylinder(_))))
    {
        return Err("SLDPRT: cached recovery currently supports planar and cylindrical faces".into());
    }
    let points: BTreeMap<_, _> = ir
        .model
        .points
        .iter()
        .map(|p| {
            let q = p.position().get();
            (p.id.clone(), [q.x, q.y, q.z])
        })
        .collect();
    let vertices: BTreeMap<_, _> = ir.model.vertices.iter().filter_map(|v| points.get(&v.point).map(|p| (v.id.clone(), *p))).collect();
    let circles: BTreeMap<_, _> = ir
        .model
        .curves
        .iter()
        .filter_map(|c| {
            if let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = &c.geometry {
                let o = circle.center().get();
                let z = circle.frame().axis().as_raw();
                let x = circle.frame().reference().as_raw();
                Some((c.id.clone(), ([o.x, o.y, o.z], [x.x, x.y, x.z], [z.x, z.y, z.z], circle.radius().get())))
            } else {
                None
            }
        })
        .collect();
    let circular_edges =
        ir.model.edges.iter().filter(|edge| edge.param_range().is_none() && edge.curve().is_some_and(|id| circles.contains_key(id))).count();
    if circular_edges.checked_mul(reference.len()).is_none_or(|n| n > 5_000_000) {
        return Err("SLDPRT: display/curve comparison exceeds the import work limit".into());
    }
    if ir.model.bodies.iter().any(|b| b.kind != cadmpeg_ir::topology::BodyKind::Solid) {
        return Err("SLDPRT: only solid bodies are supported".into());
    }
    let mut reversed = BTreeSet::new();
    for edge in &mut ir.model.edges {
        let Some((o, x, z, r)) = edge.curve().and_then(|c| circles.get(c)) else { continue };
        if edge.param_range().is_some() {
            continue;
        }
        let a = *vertices.get(&edge.start).ok_or("SLDPRT: missing circular edge start")?;
        let b = *vertices.get(&edge.end).ok_or("SLDPRT: missing circular edge end")?;
        if arc_reversed(*o, *x, *z, *r, a, b, reference)? {
            std::mem::swap(&mut edge.start, &mut edge.end);
            reversed.insert(edge.id.clone());
        }
    }
    for coedge in &mut ir.model.coedges {
        if reversed.contains(&coedge.edge) {
            coedge.sense = match coedge.sense {
                Sense::Forward => Sense::Reversed,
                Sense::Reversed => Sense::Forward,
            };
        }
    }
    let edges: BTreeMap<_, _> = ir.model.edges.iter().map(|e| (e.id.clone(), e)).collect();
    let coedges: BTreeMap<_, _> = ir.model.coedges.iter().map(|c| (c.id.clone(), c)).collect();
    let loops: BTreeMap<_, _> = ir.model.loops.iter().map(|l| (l.id.clone(), l)).collect();
    let mut boundary_work = 0usize;
    for face in &ir.model.faces {
        for id in face.loops.iter() {
            let lp = loops.get(id).ok_or("SLDPRT: missing loop")?;
            boundary_work = lp
                .coedges()
                .len()
                .checked_add(1)
                .and_then(|n| boundary_work.checked_add(n))
                .filter(|n| *n <= 16_384)
                .ok_or("SLDPRT: boundary traversal exceeds the import work limit")?;
        }
    }
    for face in &mut ir.model.faces {
        let FaceLoops::Unspecified { loops: ids } = &face.loops else { continue };
        if ids.is_empty() {
            return Err("SLDPRT: face without boundary".into());
        }
        let mut areas = Vec::with_capacity(ids.len());
        for id in ids {
            let lp = loops.get(id).ok_or("SLDPRT: missing loop")?;
            let mut polygon = Vec::new();
            for cid in lp.coedges() {
                let c = coedges.get(cid).ok_or("SLDPRT: missing coedge")?;
                let e = edges.get(&c.edge).ok_or("SLDPRT: missing edge")?;
                let v = match c.sense {
                    Sense::Forward => &e.start,
                    Sense::Reversed => &e.end,
                };
                polygon.push(*vertices.get(v).ok_or("SLDPRT: missing boundary point")?);
            }
            areas.push(polygon_area(&polygon));
        }
        let outer = outer_loop(&areas)?;
        let outer_id = ids.get(outer).ok_or("SLDPRT: missing exterior boundary")?.clone();
        let inner = ids.iter().enumerate().filter(|(i, _)| *i != outer).map(|(_, id)| id.clone()).collect();
        face.loops = FaceLoops::Classified { outer: outer_id, inner };
    }
    remove_empty_regions(ir)
}

fn remove_empty_regions(ir: &mut CadIr) -> Result<(), String> {
    use std::collections::BTreeSet;
    // Empty regions carry no faces. Keep every nonempty region and every face, then let the
    // STEP reader prove that the remaining boundaries enclose finite positive volumes.
    let empty: BTreeSet<_> = ir.model.regions.iter().filter(|r| r.shells.is_empty()).map(|r| r.id.clone()).collect();
    for body in &mut ir.model.bodies {
        body.regions.retain(|r| !empty.contains(r));
        if body.regions.is_empty() {
            return Err("SLDPRT: body has no bounded region".into());
        }
    }
    ir.model.regions.retain(|r| !empty.contains(&r.id));
    Ok(())
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
pub(super) fn polygon_area(points: &[[f64; 3]]) -> f64 {
    let Some(o) = points.first().copied() else { return 0.0 };
    let mut area = [0.0; 3];
    for (a, b) in points.iter().zip(points.iter().cycle().skip(1)).take(points.len()) {
        let v = cross(sub(*a, o), sub(*b, o));
        for (sum, value) in area.iter_mut().zip(v) {
            *sum += value
        }
    }
    dot(area, area).sqrt() * 0.5
}
pub(super) fn outer_loop(areas: &[f64]) -> Result<usize, String> {
    if areas.len() == 1 {
        return Ok(0);
    }
    let (index, best) = areas.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).ok_or("SLDPRT: no face loops")?;
    if !best.is_finite() || *best <= 1e-12 || areas.iter().enumerate().any(|(i, a)| i != index && (*best - *a).abs() <= 1e-9 * best.abs().max(1.0)) {
        return Err("SLDPRT: ambiguous face exterior".into());
    }
    Ok(index)
}
#[allow(clippy::too_many_arguments)]
pub(super) fn arc_reversed(o: [f64; 3], x: [f64; 3], z: [f64; 3], r: f64, a: [f64; 3], b: [f64; 3], reference: &[[f64; 3]]) -> Result<bool, String> {
    if !r.is_finite()
        || !(1e-12..=1e12).contains(&r)
        || [o, x, z, a, b].into_iter().flatten().chain(reference.iter().flatten().copied()).any(|v| !v.is_finite() || v.abs() > 1e12)
    {
        return Err("SLDPRT: invalid circular geometry".into());
    }
    let tau = std::f64::consts::TAU;
    let y = cross(z, x);
    let angle = |p: [f64; 3]| {
        let p = sub(p, o);
        dot(p, y).atan2(dot(p, x))
    };
    let start = angle(a);
    let span = (angle(b) - start).rem_euclid(tau);
    if span < 1e-8 || tau - span < 1e-8 {
        return Err("SLDPRT: closed circular cache edge needs an explicit range".into());
    }
    let tol = (r * 1e-5).max(1e-5);
    let angular = (tol / r).max(1e-7);
    let (mut forward, mut reverse) = (false, false);
    for p in reference {
        let q = sub(*p, o);
        let axial = dot(q, z);
        let radius = (dot(q, q) - axial * axial).max(0.0).sqrt();
        if axial.abs() > tol || (radius - r).abs() > tol {
            continue;
        }
        let t = (angle(*p) - start).rem_euclid(tau);
        if t <= angular || tau - t <= angular || (t - span).abs() <= angular {
            continue;
        }
        if t < span { forward = true } else { reverse = true }
    }
    match (forward, reverse) {
        (true, false) => Ok(false),
        (false, true) => Ok(true),
        _ => Err("SLDPRT: circular cache edge direction is not uniquely supported by display geometry".into()),
    }
}

/// A cache is accepted only when the actual SolidCraft B-rep has all faces, closed positive
/// volume, and the same physical bounds as the source display geometry.
pub(super) fn validate_step(step: &str, reference: &[[f64; 3]], reference_volume: f64) -> Result<(), String> {
    let imported = solidcraft_kernel::step_import_shared(step).map_err(|e| e.to_string())?;
    if imported.bodies.is_empty() || !imported.warnings.is_empty() {
        return Err("SLDPRT: incomplete STEP geometry".into());
    }
    let mut bounds = solidcraft_geom::Aabb3::EMPTY;
    let mut volume = 0.0;
    for body in &imported.bodies {
        let errors = body.body.validity();
        if !errors.is_empty() {
            return Err(format!("SLDPRT: invalid solid: {}", errors.join("; ")));
        }
        let m = solidcraft_kernel::measure(&body.body).map_err(|e| e.to_string())?;
        if !m.volume.is_finite() || m.volume <= 0.0 {
            return Err("SLDPRT: imported body has no finite positive volume".into());
        }
        bounds = bounds.union(&m.bbox);
        volume += m.volume;
    }
    // Display geometry is faceted. This is a consistency check, not a replacement
    // for the analytic solid: allow its approximation error, reject a different part.
    if (volume - reference_volume).abs() > reference_volume * 0.005 {
        return Err("SLDPRT: solid volume does not agree with the source display mesh".into());
    }
    let mut expected = solidcraft_geom::Aabb3::EMPTY;
    for p in reference {
        expected.add(solidcraft_geom::Vec3::new(p[0], p[1], p[2]));
    }
    let tolerance = 1e-4_f64.max((expected.max - expected.min).len() * 1e-6);
    if (bounds.min - expected.min).len() > tolerance || (bounds.max - expected.max).len() > tolerance {
        return Err("SLDPRT: imported solid bounds do not match the source display geometry".into());
    }
    Ok(())
}

pub(super) fn reference_volume(ir: &CadIr) -> Result<f64, String> {
    let mut volume = 0.0;
    let mut count = 0usize;
    let mut origin = None;
    for tess in &ir.model.tessellations {
        let vertices = tess.vertices();
        let triangles = tess.mesh().triangles();
        count = count.checked_add(triangles.len()).ok_or("SLDPRT: display mesh is too large")?;
        if count > 200_000 {
            return Err("SLDPRT: display mesh is too large".into());
        }
        for triangle in triangles {
            let mut points = [[0.0; 3]; 3];
            for (slot, index) in points.iter_mut().zip(triangle) {
                let index = usize::try_from(index).map_err(|_| "SLDPRT: invalid display index")?;
                let p = vertices.get(index).ok_or("SLDPRT: invalid display index")?.get();
                *slot = [p.x, p.y, p.z];
            }
            let o = *origin.get_or_insert(points[0]);
            volume += dot(sub(points[0], o), cross(sub(points[1], o), sub(points[2], o))) / 6.0;
        }
    }
    if !volume.is_finite() || volume.abs() < 1e-12 {
        return Err("SLDPRT: display mesh does not establish a nonzero reference volume".into());
    }
    Ok(volume.abs())
}

/// Locate unchanged imported geometry only when every active feature is metadata or that import.
pub(super) fn unchanged_import(ir: &CadIr, required: u32) -> Result<Option<String>, String> {
    if !ir.model.bodies.is_empty() || !ir.model.faces.is_empty() || !ir.model.points.is_empty() {
        return Err("SLDPRT: cache eligibility has unrelated geometry".into());
    }
    let [config] = ir.model.configurations.as_slice() else {
        return Err(format!("SLDPRT: cache eligibility needs one configuration, found {}", ir.model.configurations.len()));
    };
    if !config.active || ir.model.features.len() > 256 {
        return Err(format!("SLDPRT: cache eligibility configuration active={} features={}", config.active, ir.model.features.len()));
    }
    let Some(index) = config.source_index else { return Err("SLDPRT: cache configuration source index is missing".into()) };
    let Some(records) = ir.native.namespace("sldprt").and_then(|n| n.arenas().get("features")) else {
        return Err("SLDPRT: cache feature class records are missing".into());
    };
    let classes: BTreeMap<_, _> = records
        .iter()
        .map(|r| {
            let fields = r.fields().map_err(|e| format!("SLDPRT: cached feature metadata: {e}"))?;
            Ok((r.id().to_owned(), fields.get("input_class").and_then(|v| v.as_str()).map(str::to_owned)))
        })
        .collect::<Result<_, String>>()?;
    let mut imported = None;
    for feature in &ir.model.features {
        let active = config
            .feature_states
            .get(&feature.id)
            .map(|s| !s.evaluation.is_suppressed())
            .or_else(|| feature.suppressed.map(|suppressed| !suppressed))
            .ok_or("SLDPRT: cached recovery requires known feature suppression states")?;
        if !active {
            continue;
        }
        match feature.evaluation.definition().operation() {
            FeatureOperation::TreeNode { .. } | FeatureOperation::DatumPrincipalPlane { .. } => {}
            FeatureOperation::Native { kind, .. } if kind.as_str() == "Multi_CAD_Imported" => {
                if imported.is_some() {
                    return Err("SLDPRT: more than one active imported feature".into());
                }
                imported = feature.name.as_deref();
            }
            FeatureOperation::Native { parameters, .. } => {
                let class = feature.native_ref.as_deref().and_then(|id| classes.get(id)).and_then(Option::as_deref);
                let metadata_only =
                    class.is_none() && parameters.is_empty() && feature.dependencies.is_empty() && feature.evaluation.outputs().is_empty();
                if !metadata_only
                    && !class
                        .is_some_and(|c| c.ends_with("Folder_c") || matches!(c, "moDetailCabinet_c" | "moRefPlane_c" | "moOriginProfileFeature_c"))
                {
                    return Err(format!("SLDPRT: cached-part recovery does not support feature {:?} with class {class:?}", feature.name));
                }
            }
            _ => return Err(format!("SLDPRT: cached-part recovery does not support feature {:?}", feature.evaluation.definition().operation())),
        }
    }
    let Some(imported) = imported else { return Err("SLDPRT: unchanged imported feature is missing".into()) };
    if index != required {
        return Err("SLDPRT: imported cache configuration does not match assembly".into());
    }
    Ok(Some(imported.to_owned()))
}
