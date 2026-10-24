//! The model cache: a design's evaluated timeline (every feature's result and the state after
//! it, [`SavedModel`]), saved with it so opening need not evaluate it again. Bodies use
//! SolidCraft's exact native B-rep encoding; the key ties the cache to the exact document, build
//! and platform that computed it (docs/design-container.md).
use super::*;
use solidcraft_doc::{ModelState, SavedModel};

const FORMAT: &str = "solidcraft-model-cache/4";
const PRODUCER: &str = "solidcraft-truck-adapter/3";
const MANIFEST: &str = "cache/model.json";
const PAYLOAD: &str = "cache/bodies.bin";
/// Body records (a placed copy is one record; its part is stored once).
const MAX_RECORDS: usize = 200_000;
/// Distinct stored bodies.
const MAX_UNIQUE: usize = 100_000;
const MAX_MANIFEST: usize = 32 << 20;

/// The build that computed a cache: the same source, compiled for the same platform, computes the
/// same numbers, so only its own caches are its results.
fn build() -> String {
    format!("{}/{}-{}", env!("SOLIDCRAFT_SOURCE_COMMIT"), std::env::consts::ARCH, std::env::consts::OS)
}

/// A 4 x 4 matrix as its 16 numbers, in storage order.
fn matrix(t: &[[f64; 4]; 4]) -> Vec<f64> {
    t.iter().flatten().copied().collect()
}

/// Encode a saved model in `space` bytes (manifest and payload together). Identical bodies (the
/// same part placed many times) are stored once; a placed copy of a part records the part and its
/// matrix, so it shows that part's display as it did before saving. A model too big for `space`
/// keeps fewer states, then its final state only ([`SavedModel::thinned`], #3220); an error when
/// not even that fits.
pub(super) fn encode_model(model: &SavedModel, document: &[u8], space: usize) -> Result<(Vec<u8>, Vec<u8>)> {
    let encoded = Encoded::of(model)?;
    let total = encoded.slices.iter().fold(0usize, |n, s| n.saturating_add(s.len()));
    let fits = |manifest: &[u8], payload: &[u8]| manifest.len() <= MAX_MANIFEST && manifest.len().saturating_add(payload.len()) <= space;
    if total <= space {
        let every: Vec<usize> = (0..model.bodies.len()).collect();
        if let Ok((manifest, data)) = encoded.cache(&model.records, &every, document)
            && fits(&manifest, &data)
        {
            return Ok((manifest, data));
        }
    }
    // Fewer states. The bodies are encoded already: a state costs the slices it adds.
    let size = |s: &usize| encoded.slices.get(*s).map_or(0, Vec::len);
    let pieces: Vec<Vec<(usize, usize)>> = encoded
        .bodies
        .iter()
        .map(|(s, placed)| std::iter::once(s).chain(placed.as_ref().and_then(|(part, _)| encoded.parts.get(*part))).map(|s| (*s, size(s))).collect())
        .collect();
    let mut budget = space.min(MAX_DESIGN_BYTES);
    for _ in 0..16 {
        let Some((thin, kept, counted)) = model.thinned(&pieces, budget) else { break };
        // Only the final state, which is kept whatever it costs: nothing smaller is left to try. What
        // it counted is about what its cache holds, so over `space` it does not fit.
        let smallest = counted > budget;
        if smallest && counted > space {
            break;
        }
        let Ok((manifest, data)) = encoded.cache(&thin.records, &kept, document) else {
            if smallest {
                break;
            }
            budget = counted / 2;
            continue;
        };
        if fits(&manifest, &data) {
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
        if smallest {
            break;
        }
        // What the choice did not count (the bodies' table, the slices) took the room: choose again
        // within that much less than it counted, so each try keeps less.
        let over = manifest.len().saturating_add(data.len()).saturating_sub(space).max(manifest.len().saturating_sub(MAX_MANIFEST));
        budget = counted.saturating_sub(over).saturating_sub(1);
    }
    log::info!("the model cache ({} MB) does not fit the design file: saved without it", total >> 20);
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

impl Encoded {
    fn of(model: &SavedModel) -> Result<Self> {
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
        let encoded = solidcraft_kernel::map_on_threads(&bodies, 16, |b| solidcraft_kernel::encode_native_body(b).ok());
        let mut slices = Vec::new();
        let mut by_digest = std::collections::HashMap::new();
        let mut index = Vec::with_capacity(encoded.len());
        for bytes in encoded {
            let bytes = bytes.flatten().ok_or_else(|| invalid("body not stored exactly"))?;
            let next = slices.len();
            let i = *by_digest.entry(Sha256::digest(&bytes)).or_insert(next);
            if i == next {
                slices.push(bytes);
            }
            index.push(i);
        }
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
        if length > MAX_DESIGN_BYTES || stored.len() > MAX_UNIQUE {
            return Err(invalid("model cache size"));
        }
        let mut data = Vec::with_capacity(length);
        let mut slices = Vec::with_capacity(stored.len());
        for bytes in stored {
            slices.push(json!([data.len(), bytes.len()]));
            data.extend_from_slice(bytes);
        }
        let manifest = serde_json::to_vec(&json!({
            "format": FORMAT, "producer": PRODUCER, "app": env!("CARGO_PKG_VERSION"), "build": build(),
            "document": digest(document), "payload": digest(&data),
            "slices": slices, "shares": shares, "bodies": table, "model": records,
        }))
        .map_err(invalid)?;
        Ok((manifest, data))
    }
}

/// The final state of the model a design keeps, when this build computed it for exactly this
/// document. Any failure is a miss, never a document error.
pub fn read_design_geometry(bytes: &[u8]) -> Option<std::sync::Arc<ModelState>> {
    if bytes.len() > MAX_DESIGN_BYTES || !is_design_container(bytes) {
        return None;
    }
    DesignContainerReader::new(Cursor::new(bytes)).ok()?.geometry()?.final_state().ok()
}

/// Whether `manifest` is a model cache this build wrote for `document`.
fn current(manifest: &Value, document: &[u8]) -> bool {
    let is = |key: &str, value: &str| manifest.get(key).and_then(Value::as_str) == Some(value);
    is("format", FORMAT)
        && is("producer", PRODUCER)
        && is("app", env!("CARGO_PKG_VERSION"))
        && is("build", &build())
        && is("document", &digest(document))
        // A development build of unknown source never trusts a cache.
        && env!("SOLIDCRAFT_SOURCE_COMMIT") != "unknown"
}

pub(super) fn read_geometry_with<R: Read + Seek>(reader: &mut PackageReader<R>, document: &[u8]) -> Result<SavedModel> {
    let container: Value = serde_json::from_slice(&reader.limited_part("container.json", 4096)?).map_err(invalid)?;
    if container.get("format").and_then(Value::as_str) != Some(VERSION) {
        return Err(invalid("container version"));
    }
    let mut manifest: Value = serde_json::from_slice(&reader.limited_part(MANIFEST, MAX_MANIFEST)?).map_err(invalid)?;
    if !current(&manifest, document) {
        return Err(invalid("model cache key"));
    }
    // Validate the small parametric metadata before decoding any geometry.
    Document::from_json(std::str::from_utf8(document).map_err(invalid)?)?;
    let data = reader.limited_part(PAYLOAD, MAX_DESIGN_BYTES)?;
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
    let decoded = solidcraft_kernel::map_on_threads(&parts, 16, |bytes| solidcraft_kernel::decode_native_body(bytes).ok());
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
    let world = solidcraft_doc::world_state(&doc, &*model.final_state()?);
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
        let mesh = solidcraft_kernel::native_preview_mesh(&entry.body, detail, &mut work).map_err(invalid)?;
        triangles = triangles.saturating_add(mesh.triangles.len());
        display_bytes = display_bytes.saturating_add(mesh.positions.len().saturating_mul(24)).saturating_add(mesh.triangles.len().saturating_mul(12));
        if triangles > 1_000_000 || display_bytes > 128 << 20 {
            return Err(invalid("regenerated display budget"));
        }
        let displayed = MeshData {
            name: entry.name.clone(),
            positions: mesh.positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
            triangles: mesh.triangles.iter().flatten().copied().collect(),
            color: entry.body.color(),
        };
        validate_mesh(&displayed)?;
        preview.meshes.push(displayed);
    }
    Ok(preview)
}

pub(super) const ENTRIES: [&str; 2] = [MANIFEST, PAYLOAD];
