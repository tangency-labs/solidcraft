//! The model cache: a design's evaluated timeline (every feature's result and the state after
//! it, [`SavedModel`]), saved with it so opening need not evaluate it again. Bodies use
//! SolidCraft's exact native B-rep encoding (a mesh body, its own triangles); the key ties the
//! cache to the exact document, and to the source, platform and geometry kernel that computed it
//! (docs/design-container.md).
use super::*;
use sha2::{Digest, Sha256};
use solidcraft_doc::{ModelState, SavedModel};

const FORMAT: &str = "solidcraft-model-cache/10";
const PRODUCER: &str = "solidcraft-truck-adapter/4";
const MANIFEST: &str = "cache/model.json";
const PAYLOAD: &str = "cache/bodies.bin";
/// Body records (a placed copy is one record; its part is stored once).
const MAX_RECORDS: usize = 200_000;
/// Distinct stored bodies.
const MAX_UNIQUE: usize = 100_000;
/// The smallest cap on `cache/model.json`: what it has always been at the default design file limit.
const MIN_MANIFEST: usize = 32 << 20;

/// The cap on `cache/model.json`, saving and reading: an eighth of the design file limit in force
/// (the default 256 MiB gives the 32 MiB it always was), so a design the user allows to be that
/// large keeps its model cache (#20724). Still bounded by that limit, never unbounded.
pub(super) fn max_manifest() -> usize {
    MIN_MANIFEST.max(max_design() / 8)
}

/// The build that wrote a cache (recorded; earlier builds read only their own caches).
fn build() -> String {
    format!("{}/{}-{}", env!("SOLIDCRAFT_SOURCE_COMMIT"), std::env::consts::ARCH, std::env::consts::OS)
}

/// What computed a cache: the same evaluation source (build.rs), compiled for the same platform,
/// computes the same numbers, so only caches of that source are its results. An update that left
/// that source as it was (only the interface or the commands changed) reads the caches of the
/// build before it; one that changed it (a kernel fix, the codec) evaluates the design again.
fn evaluation() -> String {
    format!("{}/{}-{}", env!("SOLIDCRAFT_EVALUATION_SOURCE"), std::env::consts::ARCH, std::env::consts::OS)
}

/// The geometry kernel that computed a cache. A cache another kernel wrote (the C++ SMLib, which
/// earlier builds could run on) is not this build's result: the kernels differ in tolerances and in
/// what they refuse, so a design opened from it evaluates again.
pub(super) const KERNEL: &str = "solidcore";

pub(super) fn identity() -> Value {
    json!([FORMAT, PRODUCER, env!("CARGO_PKG_VERSION"), build()])
}

/// A 4 x 4 matrix as its 16 numbers, in storage order.
fn matrix(t: &[[f64; 4]; 4]) -> Vec<f64> {
    t.iter().flatten().copied().collect()
}

thread_local! {
    static LAST_CACHE_REFUSAL: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn set_last_cache_refusal(reason: String) {
    LAST_CACHE_REFUSAL.with(|r| *r.borrow_mut() = Some(reason));
}

/// The specific reason saving the model cache was refused during the last write on this thread, if any.
pub fn take_last_cache_refusal() -> Option<String> {
    LAST_CACHE_REFUSAL.with(|r| r.borrow_mut().take())
}

/// Counts bytes written without allocating a compressed output buffer.
struct ByteCount(usize);

impl std::io::Write for ByteCount {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(buf.len());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Exact Deflate-compressed byte length of `bytes` as [`PackageWriter`] stores `entry`.
/// Checks cancellation in 1 MiB chunks so a cancelled save does not stall here.
pub(super) fn deflated_len(bytes: &[u8], entry: &str) -> Result<usize> {
    use std::io::Write as _;
    let level = u32::try_from(part_level(entry, bytes.len()).clamp(0, 9)).unwrap_or(1);
    let mut encoder = flate2::write::DeflateEncoder::new(ByteCount(0), flate2::Compression::new(level));
    for piece in bytes.chunks(1 << 20) {
        if crate::safe_write::cancelled() {
            return Err(cancelled());
        }
        encoder.write_all(piece).map_err(invalid)?;
    }
    let counter = encoder.finish().map_err(invalid)?;
    Ok(counter.0)
}

/// Which size limit prevented keeping a model cache candidate.
enum FitError {
    Manifest(usize),
    Payload(usize),
    Expanded(usize),
    Compressed(usize),
}

/// Encode a saved model in `space` bytes (both compressed file budget and unpacked expanded
/// budget). See [`encode_model_with_budget`].
#[cfg(test)]
pub(super) fn encode_model(model: &SavedModel, document: &[u8], space: usize) -> Result<(Vec<u8>, Vec<u8>)> {
    encode_model_with_budget(model, document, space, space)
}

/// Encode a saved model within `file_space` compressed bytes on disk and `expanded_space`
/// uncompressed bytes in the reader's expanded-package budget (#18815). Identical bodies (the
/// same part placed many times) are stored once; a placed copy of a part records the part and its
/// matrix, so it shows that part's display as it did before saving. A model too big for either
/// budget keeps fewer states, then its final state only ([`SavedModel::thinned`], #3220); an
/// error when not even that fits.
pub(super) fn encode_model_with_budget(model: &SavedModel, document: &[u8], file_space: usize, expanded_space: usize) -> Result<(Vec<u8>, Vec<u8>)> {
    let encoded = Encoded::of(model, document)?;
    let total = encoded.slices.iter().fold(0usize, |n, s| n.saturating_add(s.len()));
    let max_payload = max_design();
    let cap = max_manifest().min(max_payload);
    let describe = |err: &FitError| match err {
        FitError::Manifest(len) => format!("its manifest is {} MB, over the {} MB allowed", len >> 20, cap >> 20),
        FitError::Payload(len) => format!("its bodies are {} MB, over the {} MB allowed", len >> 20, max_payload >> 20),
        FitError::Expanded(raw) => {
            format!("unpacked manifest and bodies are {} MB, over the {} MB unpacked room in the file", raw >> 20, expanded_space >> 20)
        }
        FitError::Compressed(compressed) => {
            format!("compressed manifest and bodies are {} MB, over the {} MB room in the file", compressed >> 20, file_space >> 20)
        }
    };
    // Check unpacked reader limits first (manifest cap, payload part limit, and expanded-package
    // limit) so the container never fails `expanded package too large` on open, then check the
    // on-disk budget against Deflate-compressed bytes (#18815).
    let check_fits = |manifest: &[u8], payload: &[u8]| -> Result<std::result::Result<(), FitError>> {
        if manifest.len() > cap {
            return Ok(Err(FitError::Manifest(manifest.len())));
        }
        if payload.len() > max_payload {
            return Ok(Err(FitError::Payload(payload.len())));
        }
        let raw = manifest.len().saturating_add(payload.len());
        if raw > expanded_space {
            return Ok(Err(FitError::Expanded(raw)));
        }
        // Fast path: even worst-case incompressible Deflate stored blocks fit within `file_space`,
        // avoiding a duplicate compression pass on ordinary saves.
        if raw.saturating_add(raw / 1000).saturating_add(64) <= file_space {
            return Ok(Ok(()));
        }
        let compressed = deflated_len(manifest, MANIFEST)?.saturating_add(deflated_len(payload, PAYLOAD)?);
        if compressed > file_space {
            return Ok(Err(FitError::Compressed(compressed)));
        }
        Ok(Ok(()))
    };
    let next_budget = |current: usize, err: &FitError| match err {
        FitError::Manifest(len) => current.saturating_sub(len.saturating_sub(cap)).saturating_sub(1),
        FitError::Payload(len) => current.saturating_sub(len.saturating_sub(max_payload)).saturating_sub(1),
        FitError::Expanded(raw) => current.saturating_sub(raw.saturating_sub(expanded_space)).saturating_sub(1),
        FitError::Compressed(compressed) => {
            let scaled = (current as u128).saturating_mul(file_space as u128) / ((*compressed).max(1) as u128);
            usize::try_from(scaled).unwrap_or(usize::MAX).min(current.saturating_sub(1))
        }
    };
    let max_raw = expanded_space.min(max_payload.saturating_add(cap));
    let mut budget = max_raw;
    let mut why: Option<String> = None;
    if total <= expanded_space.min(max_payload) {
        let every: Vec<usize> = (0..model.bodies.len()).collect();
        if let Ok((manifest, data)) = encoded.cache(&model.records, &every, document) {
            match check_fits(&manifest, &data)? {
                Ok(()) => return Ok((manifest, data)),
                Err(err) => {
                    why = Some(describe(&err));
                    let raw = manifest.len().saturating_add(data.len());
                    budget = max_raw.min(next_budget(raw, &err));
                }
            }
        }
    } else {
        why =
            Some(format!("unpacked bodies are {} MB, over the {} MB unpacked room in the file", total >> 20, expanded_space.min(max_payload) >> 20));
    }
    // Fewer states. The bodies are encoded already: a state costs the slices it adds.
    let size = |s: &usize| encoded.slices.get(*s).map_or(0, Vec::len);
    let pieces: Vec<Vec<(usize, usize)>> = encoded
        .bodies
        .iter()
        .map(|(s, placed)| std::iter::once(s).chain(placed.as_ref().and_then(|(part, _)| encoded.parts.get(*part))).map(|s| (*s, size(s))).collect())
        .collect();
    for _ in 0..16 {
        let Some((thin, kept, counted)) = model.thinned(&pieces, budget) else { break };
        // Only the final state, which is kept whatever it costs: nothing smaller is left to try.
        // Break early only when its unpacked bytes already exceed the unpacked budget; a final
        // state larger than `file_space` uncompressed may still fit once Deflate-compressed.
        let smallest = counted > budget;
        if smallest && counted > max_raw {
            break;
        }
        let Ok((manifest, data)) = encoded.cache(&thin.records, &kept, document) else {
            why = Some("it could not be encoded within the record and body limits".into());
            if smallest {
                break;
            }
            budget = counted / 2;
            continue;
        };
        match check_fits(&manifest, &data)? {
            Ok(()) => {
                let features = thin.records.get("features").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
                log::info!(
                    "the model cache ({} MB) does not fit the design file: saved the states of {} of {} features ({} MB), opening evaluates the others again",
                    total >> 20,
                    features.iter().filter(|f| f.get("state").is_some()).count(),
                    features.len(),
                    manifest.len().saturating_add(data.len()) >> 20
                );
                return Ok((manifest, data));
            }
            Err(err) => {
                why = Some(describe(&err));
                if smallest {
                    break;
                }
                // What the choice did not count (the bodies' table, the slices) or compression
                // ratio took the room: choose again within less than it counted, so each try keeps less.
                budget = next_budget(counted, &err);
            }
        }
    }
    let refusal_reason = why.as_deref().unwrap_or("not even its final state fits the design file budget");
    log::info!("the model cache ({} MB) does not fit the design file: saved without it ({refusal_reason})", total >> 20,);
    set_last_cache_refusal(refusal_reason.to_string());
    Err(invalid("model cache size"))
}

/// A model's bodies, each encoded once and identical ones stored once (keyed by the SHA-256 of
/// their encoding), in the order a cache of the whole model stores them: every body, then the part
/// every placed copy shows.
struct Encoded {
    /// Each stored body's encoding (kept apart: only what a cache keeps is copied into its payload).
    slices: Vec<Vec<u8>>,
    /// Each model body's slice, with the part (its index in `parts`) and the matrix of a placed copy.
    bodies: Vec<(usize, Option<(usize, [[f64; 4]; 4])>)>,
    /// Each placed part's slice.
    parts: Vec<usize>,
}

/// The start of a mesh body's slice. A mesh body (an imported STL or 3MF) has no B-rep for the
/// native encoding: its slice is its triangles as they are, with its colour and paint, which is
/// all a mesh body holds. A B-rep slice starts with the native encoding's own tag.
const MESH_TAG: &[u8; 8] = b"SCMESH01";
/// Most bytes of a mesh slice's header (its counts, colour and paint).
const MAX_MESH_HEADER: usize = 16 << 20;

/// A mesh body's slice: the tag, the header's length (u32) and the header (JSON: the counts,
/// colour and paint), then each vertex as three f64 and each triangle as three u32, little-endian.
fn encode_mesh_body(body: &solidcraft_kernel::Body) -> Option<Vec<u8>> {
    let (positions, triangles) = body.triangle_mesh()?;
    let normals = body.triangle_corner_normals()?;
    let header =
        serde_json::to_vec(&json!({"positions": positions.len(), "triangles": triangles.len(), "corner_normals": normals.len(), "color": body.color(), "paint": body.paint()}))
            .ok()?;
    if header.len() > MAX_MESH_HEADER {
        return None;
    }
    let size = positions
        .len()
        .checked_mul(24)?
        .checked_add(triangles.len().checked_mul(12)?)?
        .checked_add(normals.len().checked_mul(72)?)?
        .checked_add(12 + header.len())?;
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(MESH_TAG);
    out.extend_from_slice(&u32::try_from(header.len()).ok()?.to_le_bytes());
    out.extend_from_slice(&header);
    for p in positions {
        for x in [p.x, p.y, p.z] {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    for t in triangles {
        for i in t {
            out.extend_from_slice(&i.to_le_bytes());
        }
    }
    for n in normals.iter().flatten() {
        for x in [n.x, n.y, n.z] {
            out.extend_from_slice(&x.to_le_bytes());
        }
    }
    Some(out)
}

/// The mesh body of a slice [`encode_mesh_body`] wrote; `None` for anything else.
fn decode_mesh_body(bytes: &[u8]) -> Option<solidcraft_kernel::Body> {
    let rest = bytes.strip_prefix(MESH_TAG.as_slice())?;
    let (length, rest) = rest.split_first_chunk::<4>()?;
    let length = usize::try_from(u32::from_le_bytes(*length)).ok().filter(|n| *n <= MAX_MESH_HEADER)?;
    let (header, data) = rest.split_at_checked(length)?;
    let header: Value = serde_json::from_slice(header).ok()?;
    let count = |key: &str| header.get(key).and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok());
    let (vertices, triangles) = (count("positions")?, count("triangles")?);
    let normals = count("corner_normals").unwrap_or(0);
    if normals != 0 && normals != triangles {
        return None;
    }
    if triangles > solidcraft_kernel::MAX_TRIANGLES {
        return None;
    }
    // The counts must account for every byte, so nothing is allocated for data that is not there.
    let split = vertices.checked_mul(24)?;
    let normal_start = split.checked_add(triangles.checked_mul(12)?)?;
    if normal_start.checked_add(normals.checked_mul(72)?)? != data.len() {
        return None;
    }
    let (coordinates, rest) = data.split_at_checked(split)?;
    let (corners, shading) = rest.split_at_checked(triangles.checked_mul(12)?)?;
    let f64_at = |c: &[u8], at: usize| c.get(at..at + 8)?.try_into().ok().map(f64::from_le_bytes);
    let u32_at = |c: &[u8], at: usize| c.get(at..at + 4)?.try_into().ok().map(u32::from_le_bytes);
    let positions: Vec<solidcore_record::Vec3> = coordinates
        .as_chunks::<24>()
        .0
        .iter()
        .map(|c| Some(solidcore_record::Vec3::new(f64_at(c, 0)?, f64_at(c, 8)?, f64_at(c, 16)?)))
        .collect::<Option<_>>()?;
    let triangles: Vec<[u32; 3]> =
        corners.as_chunks::<12>().0.iter().map(|c| Some([u32_at(c, 0)?, u32_at(c, 4)?, u32_at(c, 8)?])).collect::<Option<_>>()?;
    let normals: Vec<[solidcore_record::Vec3; 3]> = shading
        .as_chunks::<72>()
        .0
        .iter()
        .map(|c| {
            let n = |offset| Some(solidcore_record::Vec3::new(f64_at(c, offset)?, f64_at(c, offset + 8)?, f64_at(c, offset + 16)?));
            Some([n(0)?, n(24)?, n(48)?])
        })
        .collect::<Option<_>>()?;
    let color: Option<[f32; 3]> = serde_json::from_value(header.get("color").cloned().unwrap_or(Value::Null)).ok()?;
    let paint: Option<solidcraft_kernel::Paint> = serde_json::from_value(header.get("paint").cloned().unwrap_or(Value::Null)).ok()?;
    if color.is_some_and(|c| c.iter().any(|x| !x.is_finite())) {
        return None;
    }
    Some(solidcraft_kernel::mesh_body_with_normals(&positions, &triangles, &normals).ok()?.with_color(color).with_paint(paint))
}

/// A mesh body decoded is the one encoded: the same vertices, bit for bit, the same triangles,
/// colour and paint.
fn same_mesh(a: &solidcraft_kernel::Body, b: &solidcraft_kernel::Body) -> bool {
    let (Some((pa, ta)), Some((pb, tb))) = (a.triangle_mesh(), b.triangle_mesh()) else { return false };
    let bits = |p: &[solidcore_record::Vec3]| p.iter().map(|v| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]).collect::<Vec<_>>();
    let normals = |body: &solidcraft_kernel::Body| {
        body.triangle_corner_normals().unwrap_or_default().iter().flatten().map(|n| [n.x.to_bits(), n.y.to_bits(), n.z.to_bits()]).collect::<Vec<_>>()
    };
    ta == tb && bits(pa) == bits(pb) && normals(a) == normals(b) && a.color() == b.color() && a.paint() == b.paint()
}

/// A body of the cache's payload, mesh or B-rep by the slice's tag.
fn decode_cached_body(bytes: &[u8]) -> Option<solidcraft_kernel::Body> {
    if bytes.starts_with(MESH_TAG) { decode_mesh_body(bytes) } else { solidcraft_kernel::decode_native_body(bytes).ok() }
}

/// Pre-encode identity key for deduplicating identical bodies (same record/mesh, size, colour,
/// paint and encoded metadata pointers) before running native encode + round-trip decode + SHA-256.
/// Paired with [`solidcraft_kernel::Body::same`] so differences in boundary protection are never
/// collapsed.
#[derive(PartialEq, Eq, Hash)]
struct BodyEncodeKey<'a> {
    is_mesh: bool,
    record_ptr: usize,
    mesh_ptr: usize,
    size_bits: u64,
    color_bits: Option<[u32; 3]>,
    paint_bits: Option<(u32, Option<&'a str>, Vec<(usize, [u32; 3], u32, Option<&'a str>)>)>,
    splits_ptr: usize,
    face_sources_ptr: usize,
    history_ptr: usize,
}

impl<'a> BodyEncodeKey<'a> {
    fn of(b: &'a solidcraft_kernel::Body) -> Self {
        Self {
            is_mesh: b.is_mesh(),
            record_ptr: std::ptr::from_ref(b.record()).addr(),
            mesh_ptr: b.triangle_mesh().map_or(0, |(p, _)| p.as_ptr() as usize),
            size_bits: b.size().to_bits(),
            color_bits: b.color().map(|[r, g, bl]| [r.to_bits(), g.to_bits(), bl.to_bits()]),
            paint_bits: b.paint().map(|p| {
                (
                    p.opacity.to_bits(),
                    p.name.as_deref(),
                    p.faces
                        .iter()
                        .map(|f| (f.face, [f.color[0].to_bits(), f.color[1].to_bits(), f.color[2].to_bits()], f.opacity.to_bits(), f.name.as_deref()))
                        .collect(),
                )
            }),
            splits_ptr: b.split_lines().map_or(0, |s| std::sync::Arc::as_ptr(s) as usize),
            face_sources_ptr: b.face_sources().map_or(0, |s| s.as_ptr() as usize),
            history_ptr: b.history().map_or(0, |h| std::ptr::from_ref(h).addr()),
        }
    }
}

impl Encoded {
    fn of(model: &SavedModel, document: &[u8]) -> Result<Self> {
        // What to encode: every body, and the part every placed copy shows.
        let mut shares: Vec<solidcraft_kernel::DisplayShare> = Vec::new();
        let mut share_index = std::collections::HashMap::new();
        let mut placed = Vec::with_capacity(model.bodies.len());
        for b in &model.bodies {
            placed.push(b.display_share().map(|(share, t)| {
                let part = *share_index.entry(share.id()).or_insert_with(|| {
                    shares.push(share.clone());
                    shares.len() - 1
                });
                (part, t)
            }));
        }
        let bodies: Vec<&solidcraft_kernel::Body> = model.bodies.iter().chain(shares.iter().map(|s| s.original())).collect();
        // Deduplicate identical bodies before encoding, round-trip decoding and hashing: repeated
        // body instances (and shared display parts) then encode once instead of once per record,
        // while preserving first-seen slice order and refusal attribution byte for byte.
        let mut unique_bodies: Vec<&solidcraft_kernel::Body> = Vec::new();
        let mut unique_first_idx: Vec<usize> = Vec::new();
        let mut body_to_unique: Vec<usize> = Vec::with_capacity(bodies.len());
        {
            let mut by_key: std::collections::HashMap<BodyEncodeKey<'_>, Vec<usize>> = std::collections::HashMap::with_capacity(bodies.len());
            for (idx, &b) in bodies.iter().enumerate() {
                let bucket = by_key.entry(BodyEncodeKey::of(b)).or_default();
                let found = bucket.iter().copied().find(|&u| unique_bodies.get(u).is_some_and(|other| b.same(other)));
                let u = match found {
                    Some(existing) => existing,
                    None => {
                        let u = unique_bodies.len();
                        unique_bodies.push(b);
                        unique_first_idx.push(idx);
                        bucket.push(u);
                        u
                    }
                };
                body_to_unique.push(u);
            }
        }
        #[cfg(test)]
        ENCODED_UNIQUE.with(|n| n.set(n.get() + unique_bodies.len()));
        // A body is kept only when it decodes to itself: one the codec gave back with other
        // topology (two loose vertices more on a pattern copy cut by an extrude) would open as
        // another body. Then the model isn't kept, and opening the design evaluates it.
        let encoded = solidcraft_kernel::map_on_threads(&unique_bodies, 16, |b| {
            if b.is_mesh() {
                let Some(bytes) = encode_mesh_body(b) else {
                    return Err("mesh body could not be encoded".into());
                };
                let Some(back) = decode_mesh_body(&bytes) else {
                    return Err("mesh body could not be decoded back".into());
                };
                if !same_mesh(b, &back) {
                    return Err("mesh body differed on round-trip".into());
                }
                return Ok(bytes);
            }
            let bytes = solidcraft_kernel::encode_native_body(b).map_err(|e| format!("native body encoding failed: {e}"))?;
            let back = solidcraft_kernel::decode_native_body(&bytes).map_err(|e| format!("native body decoding failed: {e}"))?;
            let (fa, ea, va, sa) = (b.face_count(), b.edge_count(), b.vertex_count(), b.shell_count());
            let (fb, eb, vb, sb) = (back.face_count(), back.edge_count(), back.vertex_count(), back.shell_count());
            if (fa, ea, va, sa) != (fb, eb, vb, sb) {
                return Err(format!(
                    "body differed on round-trip: decoded with {fb} faces, {eb} edges, {vb} vertices vs original {fa} faces, {ea} edges, {va} vertices"
                ));
            }
            Ok(bytes)
        });
        let mut slices = Vec::new();
        let mut by_digest = std::collections::HashMap::new();
        let mut unique_slices = Vec::with_capacity(encoded.len());
        for (u, res) in encoded.into_iter().enumerate() {
            let detail = match res {
                Some(Ok(bytes)) => {
                    let next = slices.len();
                    let i = *by_digest.entry(Sha256::digest(&bytes)).or_insert(next);
                    if i == next {
                        slices.push(bytes);
                    }
                    unique_slices.push(i);
                    continue;
                }
                Some(Err(d)) => d,
                None => "body encoding worker failed".to_string(),
            };
            let idx = unique_first_idx.get(u).copied().unwrap_or(u);
            let body_index = if idx < model.bodies.len() {
                Some(idx)
            } else {
                let share_idx = idx - model.bodies.len();
                placed.iter().position(|p| p.as_ref().map(|(part, _)| *part == share_idx).unwrap_or(false))
            };
            let (body_name, feature_id) = body_index
                .and_then(|bi| {
                    let model_bodies = model.records.get("model_bodies")?.as_array()?;
                    for entry in model_bodies {
                        if entry.get("body").and_then(Value::as_u64) == Some(bi as u64) {
                            let name = entry.get("name").and_then(Value::as_str).map(String::from);
                            let feat = entry.get("feature").and_then(Value::as_u64);
                            return Some((name, feat));
                        }
                    }
                    None
                })
                .unwrap_or((None, None));
            let body_name = body_name.filter(|s| !s.is_empty());
            let feature_name = feature_id
                .and_then(|fid| {
                    let doc_val = serde_json::from_slice::<Value>(document).ok()?;
                    let features = doc_val.get("features")?.as_array()?;
                    for f in features {
                        if f.get("id").and_then(Value::as_u64) == Some(fid) {
                            return f.get("name").and_then(Value::as_str).map(String::from);
                        }
                    }
                    None
                })
                .filter(|s| !s.is_empty());
            let refusal = match (&body_name, &feature_name) {
                (Some(b), Some(f)) => format!("body '{b}' in feature '{f}' could not be stored exactly: {detail}"),
                (Some(b), None) => format!("body '{b}' could not be stored exactly: {detail}"),
                (None, Some(f)) => format!("a body in feature '{f}' could not be stored exactly: {detail}"),
                (None, None) => format!("a body could not be stored exactly: {detail}"),
            };
            log::info!("the model cache could not be saved: {refusal}");
            set_last_cache_refusal(refusal.clone());
            return Err(invalid(refusal));
        }
        let index: Vec<usize> = body_to_unique.iter().filter_map(|&u| unique_slices.get(u).copied()).collect();
        let (body_slices, part_slices) = index.split_at_checked(model.bodies.len()).ok_or_else(|| invalid("cached body count"))?;
        Ok(Encoded { slices, bodies: body_slices.iter().copied().zip(placed).collect(), parts: part_slices.to_vec() })
    }

    /// The cache of a model with these `records`, whose bodies are these of the model's (indices
    /// here): its manifest and payload. Refused when it holds too many bodies or its payload is
    /// too large.
    fn cache(&self, records: &Value, bodies: &[usize], document: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
        if bodies.is_empty() || bodies.len() > MAX_RECORDS {
            return Err(invalid("cached body count"));
        }
        // The slices stored, in the order first needed: the bodies', then their parts'.
        let mut order = Vec::new();
        let mut renumbered = vec![None; self.slices.len()];
        let mut slice = |s: usize| -> Result<usize> {
            let n = renumbered.get_mut(s).ok_or_else(|| invalid("cached body"))?;
            Ok(*n.get_or_insert_with(|| {
                order.push(s);
                order.len() - 1
            }))
        };
        let mut parts = Vec::new();
        let mut part_index = vec![None; self.parts.len()];
        let mut table = Vec::with_capacity(bodies.len());
        for &b in bodies {
            let (s, placed) = self.bodies.get(b).ok_or_else(|| invalid("cached body"))?;
            let s = slice(*s)?;
            table.push(match placed {
                Some((part, t)) => {
                    let p = *part_index.get_mut(*part).ok_or_else(|| invalid("display share"))?.get_or_insert_with(|| {
                        parts.push(*part);
                        parts.len() - 1
                    });
                    json!([s, p, matrix(t)])
                }
                None => json!([s]),
            });
        }
        let shares = parts.iter().map(|p| slice(*self.parts.get(*p).ok_or_else(|| invalid("display share"))?)).collect::<Result<Vec<_>>>()?;
        let stored = order.iter().map(|s| self.slices.get(*s).ok_or_else(|| invalid("cached body"))).collect::<Result<Vec<_>>>()?;
        let length = stored.iter().fold(0usize, |n, bytes| n.saturating_add(bytes.len()));
        if length > max_design() || stored.len() > MAX_UNIQUE {
            return Err(invalid("model cache size"));
        }
        let mut data = Vec::with_capacity(length);
        let mut slices = Vec::with_capacity(stored.len());
        for bytes in stored {
            slices.push(json!([data.len(), bytes.len()]));
            data.extend_from_slice(bytes);
        }
        let manifest = serde_json::to_vec(&Manifest {
            app: env!("CARGO_PKG_VERSION"),
            bodies: table,
            build: build(),
            document: digest(document),
            evaluation: evaluation(),
            format: FORMAT,
            kernel: KERNEL,
            model: records,
            payload: digest(&data),
            producer: PRODUCER,
            shares,
            slices,
        })
        .map_err(invalid)?;
        Ok((manifest, data))
    }
}

/// A model cache's manifest as written: the model's records are written where they are, not
/// copied into a JSON tree first (copying a large model's records took as long as writing them,
/// and a cache too big for the file is written again for each try at fewer states). The fields
/// are in the order a JSON object keeps its keys (sorted), so the manifest is the one a tree
/// wrote, byte for byte.
#[derive(serde::Serialize)]
struct Manifest<'a> {
    app: &'a str,
    bodies: Vec<Value>,
    build: String,
    document: String,
    evaluation: String,
    format: &'a str,
    kernel: &'a str,
    model: &'a Value,
    payload: String,
    producer: &'a str,
    shares: Vec<usize>,
    slices: Vec<Value>,
}

/// The final state of the model a design keeps, when this build's evaluation source computed it
/// for exactly this document. Any failure is a miss, never a document error.
pub fn read_design_geometry(bytes: &[u8]) -> Option<std::sync::Arc<ModelState>> {
    if bytes.len() > max_design() || !is_design_container(bytes) {
        return None;
    }
    DesignContainerReader::new(Cursor::new(bytes)).ok()?.geometry()?.final_state().ok()
}

/// Whether `manifest` is a model cache of this build's evaluation source and kernel for `document`.
/// The application version and commit that wrote it are not asked: another version of the same
/// evaluation source computed the same model.
fn current(manifest: &Value, document: &[u8]) -> bool {
    let is = |key: &str, value: &str| manifest.get(key).and_then(Value::as_str) == Some(value);
    is("format", FORMAT)
        && is("producer", PRODUCER)
        && is("evaluation", &evaluation())
        && is("kernel", KERNEL)
        && is("document", &digest(document))
        && source_trusted(env!("SOLIDCRAFT_EVALUATION_SOURCE"), cfg!(any(test, feature = "trust-unknown-source")))
}

#[cfg(test)]
thread_local! {
    /// How often reading a model cache validated its document (tests count it).
    pub(super) static VALIDATED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// How many unique bodies `Encoded::of` sent through native/mesh encoding (tests count it).
    pub(super) static ENCODED_UNIQUE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A build whose evaluation source could not be hashed (`"unknown"`) never trusts a cache,
/// unless `in_tests` (the `trust-unknown-source` feature is only enabled by dev-dependencies).
pub(super) fn source_trusted(source: &str, in_tests: bool) -> bool {
    source != "unknown" || in_tests
}

pub(super) fn read_geometry_with<R: Read + Seek>(reader: &mut PackageReader<R>, document: &[u8]) -> Result<SavedModel> {
    read_geometry(reader, document, true)
}

/// [`read_geometry_with`] for a reader whose document is read next: reading it runs the same
/// migrations and validation and fails the open when they fail, so validating it here as well
/// parsed and validated a large design twice (the model is dropped with the failed open).
pub(super) fn read_geometry_before_document<R: Read + Seek>(reader: &mut PackageReader<R>, document: &[u8]) -> Result<SavedModel> {
    read_geometry(reader, document, false)
}

fn read_geometry<R: Read + Seek>(reader: &mut PackageReader<R>, document: &[u8], validate: bool) -> Result<SavedModel> {
    let container: Value = serde_json::from_slice(&reader.limited_part("container.json", 4096)?).map_err(invalid)?;
    if container.get("format").and_then(Value::as_str) != Some(VERSION) {
        return Err(invalid("container version"));
    }
    let mut manifest: Value = serde_json::from_slice(&reader.limited_part(MANIFEST, max_manifest())?).map_err(invalid)?;
    if !current(&manifest, document) {
        return Err(invalid("model cache key"));
    }
    // Validate the small parametric metadata before decoding any geometry.
    if validate {
        #[cfg(test)]
        VALIDATED.with(|n| n.set(n.get() + 1));
        Document::from_json(std::str::from_utf8(document).map_err(invalid)?)?;
    }
    let data = reader.limited_part(PAYLOAD, max_design())?;
    if manifest.get("payload").and_then(Value::as_str) != Some(digest(&data).as_str()) {
        return Err(invalid("model cache payload key"));
    }
    let count = |v: Option<&Value>| v.and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()).ok_or_else(|| invalid("model cache number"));
    let slices = manifest
        .get("slices")
        .and_then(Value::as_array)
        .filter(|b| !b.is_empty() && b.len() <= MAX_UNIQUE)
        .ok_or_else(|| invalid("cached bodies"))?;
    let mut end = 0usize;
    let mut parts = Vec::with_capacity(slices.len());
    for s in slices {
        let (offset, length) = (count(s.get(0))?, count(s.get(1))?);
        if offset != end {
            return Err(invalid("overlapping cached bodies"));
        }
        end = end.checked_add(length).ok_or_else(|| invalid("cached body length"))?;
        parts.push(data.get(offset..end).ok_or_else(|| invalid("truncated cached body"))?);
    }
    if end != data.len() {
        return Err(invalid("trailing cached data"));
    }
    let decoded = solidcraft_kernel::map_on_threads(&parts, 16, |bytes| decode_cached_body(bytes));
    let unique = decoded.into_iter().map(Option::flatten).collect::<Option<Vec<_>>>().ok_or_else(|| invalid("cached body"))?;
    let body = |v: Option<&Value>| unique.get(count(v)?).ok_or_else(|| invalid("cached body index"));
    let shares = manifest
        .get("shares")
        .and_then(Value::as_array)
        .filter(|s| s.len() <= MAX_UNIQUE)
        .ok_or_else(|| invalid("cached shares"))?
        .iter()
        .map(|i| Ok(solidcraft_kernel::DisplayShare::new(body(Some(i))?.clone())))
        .collect::<Result<Vec<_>>>()?;
    let table = manifest
        .get("bodies")
        .and_then(Value::as_array)
        .filter(|s| !s.is_empty() && s.len() <= MAX_RECORDS)
        .ok_or_else(|| invalid("cached body table"))?;
    let mut bodies = Vec::with_capacity(table.len());
    for entry in table {
        let mut b = body(entry.get(0))?.clone();
        if let Some(share) = entry.get(1) {
            let s = shares.get(count(Some(share))?).ok_or_else(|| invalid("cached share index"))?;
            let numbers: Vec<f64> = serde_json::from_value(entry.get(2).cloned().unwrap_or(Value::Null)).map_err(invalid)?;
            b = b.with_display_share(s, matrix_of(&numbers)?);
        }
        bodies.push(b);
    }
    let records = manifest.get_mut("model").map(Value::take).ok_or_else(|| invalid("cached model"))?;
    Ok(SavedModel { records, bodies })
}

/// A 4 x 4 matrix from its 16 numbers in storage order, finite.
fn matrix_of(numbers: &[f64]) -> Result<[[f64; 4]; 4]> {
    if numbers.len() != 16 || numbers.iter().any(|x| !x.is_finite() || x.abs() > 1e12) {
        return Err(invalid("cached matrix"));
    }
    let mut t = [[0.0; 4]; 4];
    for (slot, x) in t.iter_mut().flatten().zip(numbers) {
        *slot = *x;
    }
    Ok(t)
}

pub(super) fn geometry_preview(document: &[u8], model: &SavedModel) -> Result<DesignPreview> {
    let doc = Document::from_json(std::str::from_utf8(document).map_err(invalid)?)?;
    let world = solidcraft_doc::world_state(&doc, &*model.final_state()?)?;
    let mut preview = DesignPreview::default();
    let mut triangles = 0usize;
    let mut display_bytes = 0usize;
    let faces: usize = world.bodies.iter().map(|b| b.body.face_count()).sum();
    let detail = if faces > 20_000 {
        1
    } else if faces > 4_000 {
        4
    } else {
        8
    };
    let mut work = 1_000_000usize;
    for entry in &world.bodies {
        // A mesh body is drawn with its own triangles; a B-rep body with a coarse mesh of it.
        let made;
        let (positions, faces) = match entry.body.triangle_mesh() {
            Some(own) => own,
            None => {
                made = solidcraft_kernel::native_preview_mesh(&entry.body, detail, &mut work).map_err(invalid)?;
                (made.positions.as_slice(), made.triangles.as_slice())
            }
        };
        triangles = triangles.saturating_add(faces.len());
        display_bytes = display_bytes.saturating_add(positions.len().saturating_mul(24)).saturating_add(faces.len().saturating_mul(84));
        if triangles > 1_000_000 || display_bytes > 128 << 20 {
            return Err(invalid("regenerated display budget"));
        }
        let displayed = MeshData {
            name: entry.name.clone(),
            positions: positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
            triangles: faces.iter().flatten().copied().collect(),
            color: entry.body.color(),
            corner_normals: entry.body.triangle_corner_normals().unwrap_or_default().iter().flatten().flat_map(|n| [n.x, n.y, n.z]).collect(),
        };
        validate_mesh(&displayed)?;
        preview.meshes.push(displayed);
    }
    Ok(preview)
}

pub(super) const ENTRIES: [&str; 2] = [MANIFEST, PAYLOAD];
