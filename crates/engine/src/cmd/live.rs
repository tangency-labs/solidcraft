//! `live.meshes`: the visible bodies as welded polygon meshes for a program that follows the open
//! design (a live link to a modelling or rendering program). See `docs/formats/live-mesh.md` and
//! "Live link" in `docs/control-protocol.md`.
//!
//! Each body is meshed in its definition's place (a part placed many times is meshed once) from
//! its display plan, then joined into polygons (`solidcraft_geom::poly_mesh`) and encoded
//! (`solidcraft_io::live_mesh`). The command keeps no state about its caller: every reply lists
//! every object, and the caller says which meshes it holds already (`have`). Encoded meshes are
//! kept in a small cache keyed by the definition's geometry and the options, so a reply after a
//! change re-meshes only what changed.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use solidcraft_doc::ModelBody;
use solidcraft_doc::display_quality::DisplayQuality;
use solidcraft_geom::poly_mesh::{PolyMesh, PolyOptions, PolyTopology};
use solidcraft_io::live_mesh::{LiveMesh, encode};
use solidcraft_kernel::Body;

use super::CommandSpec;
use crate::params::bad;
use crate::schema::live::{LiveComponent, LiveFaceLook, LiveMeshesParams, LiveMeshesResult, LiveObject, LiveQuality, LiveTessellation, LiveTopology};
use crate::schema::{TypedContract, output_schema_for};
use crate::{EngineError, Result, Session};

const CMD: &str = "live.meshes";
const CONTRACT: TypedContract<LiveMeshesParams> = TypedContract::new();

/// Most names in `bodies` or keys in `have`.
const MAX_NAMES: usize = 1 << 16;
/// Most entries in `overrides`.
const MAX_OVERRIDES: usize = 4096;
/// Most encoded meshes kept, and their total size.
const CACHE_ENTRIES: usize = 64;
const CACHE_BYTES: usize = 512 << 20;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Live Link Meshes", crate::checked!("live.meshes", run))
    .icon("inspect")
    .noundo()
    .contract(&CONTRACT)
    .output(output_schema_for::<LiveMeshesResult>)
    .params(
        "quality?: coarse|normal|fine|ultra, chord_mm?|chord_relative?, angle_deg?, topology?: ngons|quads|triangles, normals?: bool, bodies?: [name], overrides?: [{body, quality?, chord_mm?, chord_relative?, angle_deg?, topology?}], have?: [mesh key], include_hidden?: bool, max_mb?: number (default 64) → {document_id, unit, up, components, objects: [{key, definition, component, matrix (column-major), mesh, visible, color, opacity, faces?}], meshes: [live-link mesh (docs/formats/live-mesh.md)], pending?}. Read-only; replies can be many megabytes, so it is meant for a live link, not an agent's context",
    )];

/// How one body is meshed, resolved from the parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Settings {
    quality: DisplayQuality,
    /// `(chord in mm or as a fraction of the body's size, relative?)`; `None`: from `quality`.
    chord: Option<(f64, bool)>,
    /// Radians; `None`: from `quality`.
    angle: Option<f64>,
    topology: PolyTopology,
    normals: bool,
}

impl Settings {
    /// What goes into a mesh's key: everything that changes its polygons.
    fn tag(&self) -> String {
        let topology = match self.topology {
            PolyTopology::NGons => "ngons",
            PolyTopology::Quads => "quads",
            PolyTopology::Triangles => "triangles",
        };
        let chord = match self.chord {
            Some((c, true)) => format!("r{c:e}"),
            Some((c, false)) => format!("mm{c:e}"),
            None => self.quality.id().to_string(),
        };
        let angle = self.angle.map_or_else(String::new, |a| format!("{a:e}"));
        format!("{topology}/{chord}/{angle}")
    }

    /// The plan made the way the viewport makes it (cached with the body).
    fn uses_display_plan(&self) -> bool {
        self.chord.is_none() && self.angle.is_none()
    }
}

fn settings(t: &LiveTessellation, base: Option<&Settings>, normals: bool) -> Result<Settings> {
    let finite = |v: f64, what: &str, lo: f64, hi: f64| -> Result<f64> {
        if v.is_finite() && v > lo && v <= hi { Ok(v) } else { Err(bad(CMD, format!("{what} must be more than {lo} and at most {hi}, got {v}"))) }
    };
    let quality = match t.quality {
        Some(LiveQuality::Coarse) => DisplayQuality::Coarse,
        Some(LiveQuality::Normal) => DisplayQuality::Normal,
        Some(LiveQuality::Fine) => DisplayQuality::Fine,
        Some(LiveQuality::Ultra) => DisplayQuality::Ultra,
        None => base.map_or(DisplayQuality::Normal, |b| b.quality),
    };
    let chord = match (t.chord_mm, t.chord_relative) {
        (Some(mm), _) => Some((finite(mm, "chord_mm", 0.0, 1e6)?, false)),
        (None, Some(r)) => Some((finite(r, "chord_relative", 0.0, 0.1)?, true)),
        // A quality given here replaces the base's own chord.
        (None, None) if t.quality.is_some() => None,
        (None, None) => base.and_then(|b| b.chord),
    };
    let angle = match t.angle_deg {
        Some(a) => Some(finite(a, "angle_deg", 0.0, 90.0)?.max(1.0).to_radians()),
        None if t.quality.is_some() => None,
        None => base.and_then(|b| b.angle),
    };
    let topology = match t.topology {
        Some(LiveTopology::Ngons) => PolyTopology::NGons,
        Some(LiveTopology::Quads) => PolyTopology::Quads,
        Some(LiveTopology::Triangles) => PolyTopology::Triangles,
        None => base.map_or(PolyTopology::NGons, |b| b.topology),
    };
    Ok(Settings { quality, chord, angle, topology, normals })
}

/// An encoded mesh of a definition body made with some settings.
struct Cached {
    body: Body,
    tag: String,
    mesh: Arc<LiveMesh>,
    fallback: bool,
}

/// Recently encoded meshes, newest last.
static CACHE: Mutex<Vec<Cached>> = Mutex::new(Vec::new());

fn cached(body: &Body, tag: &str) -> Option<(Arc<LiveMesh>, bool)> {
    let cache = CACHE.lock().ok()?;
    cache.iter().rev().find(|c| c.tag == tag && c.body.same(body)).map(|c| (c.mesh.clone(), c.fallback))
}

fn remember(body: &Body, tag: String, mesh: Arc<LiveMesh>, fallback: bool) {
    let Ok(mut cache) = CACHE.lock() else { return };
    cache.retain(|c| !(c.tag == tag && c.body.same(body)));
    cache.push(Cached { body: body.clone(), tag, mesh, fallback });
    let mut total: usize = cache.iter().map(|c| c.mesh.bytes).sum();
    while cache.len() > CACHE_ENTRIES || (total > CACHE_BYTES && cache.len() > 1) {
        let gone = cache.remove(0);
        total = total.saturating_sub(gone.mesh.bytes);
    }
}

/// The encoded mesh of `b`'s definition with `settings`: `(mesh, made from triangles)`.
fn mesh_of(b: &ModelBody, settings: &Settings) -> std::result::Result<(Arc<LiveMesh>, bool), String> {
    let (definition, _) = b.definition_in_place();
    let tag = settings.tag();
    if let Some(hit) = cached(definition, &tag) {
        return Ok(hit);
    }
    let options = PolyOptions { topology: settings.topology, ..PolyOptions::default() };
    let planned = if settings.uses_display_plan() {
        b.plan_in_place_at(settings.quality).zip(b.plan_mesh_in_place_at(settings.quality)).map(|((plan, _), (mesh, _))| (plan, mesh))
    } else {
        let size = definition.size();
        let chord = match settings.chord {
            Some((r, true)) => r * size,
            Some((mm, false)) => mm,
            None => settings.quality.chord(size),
        };
        let angle = settings.angle.unwrap_or_else(|| settings.quality.angle());
        definition.display_plan_angular(chord, angle).ok().and_then(|plan| {
            let mesh = definition.display_mesh_of(&plan).ok()?;
            Some((Arc::new(plan), Arc::new(mesh)))
        })
    };
    let (poly, fallback) = match planned {
        Some((plan, mesh)) => (PolyMesh::from_plan(&plan, &mesh, &options), false),
        // A mesh body, or one the planner refuses: its reference triangles.
        None => (PolyMesh::from_mesh(&b.mesh_in_place().0, &options), true),
    };
    let poly = poly.map_err(|e| e.to_string())?;
    let names = solidcraft_doc::naming::face_names(b);
    let mesh = Arc::new(encode(&poly, &tag, &names, settings.normals));
    remember(definition, tag, mesh.clone(), fallback);
    Ok((mesh, fallback))
}

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    let params: LiveMeshesParams = serde_json::from_value(p.clone()).map_err(|e| bad(CMD, e.to_string()))?;
    let normals = params.normals.unwrap_or(true);
    let base = settings(&params.tessellation, None, normals)?;
    let bodies = params.bodies.unwrap_or_default();
    let overrides = params.overrides.unwrap_or_default();
    let have = params.have.unwrap_or_default();
    if bodies.len() > MAX_NAMES || have.len() > MAX_NAMES {
        return Err(bad(CMD, format!("bodies and have may name at most {MAX_NAMES} each")));
    }
    if overrides.len() > MAX_OVERRIDES {
        return Err(bad(CMD, format!("overrides may name at most {MAX_OVERRIDES} bodies")));
    }
    let mut per_body: BTreeMap<String, Settings> = BTreeMap::new();
    for o in &overrides {
        per_body.insert(o.body.clone(), settings(&o.tessellation, Some(&base), normals)?);
    }
    let max_mb = match params.max_mb {
        Some(mb) if mb.is_finite() => mb.clamp(1.0, 512.0),
        Some(mb) => return Err(bad(CMD, format!("max_mb must be a number from 1 to 512, got {mb}"))),
        None => 64.0,
    };
    let budget = (max_mb * f64::from(1u32 << 20)) as usize;
    let include_hidden = params.include_hidden.unwrap_or(false);
    let only: HashSet<&str> = bodies.iter().map(String::as_str).collect();
    let have: HashSet<&str> = have.iter().map(String::as_str).collect();

    let world = s.world_state();
    let mut objects = Vec::new();
    let mut meshes = Vec::new();
    let mut pending = Vec::new();
    let mut sent: HashSet<String> = HashSet::new();
    let mut size = 0usize;
    for b in world.bodies.iter() {
        if !only.is_empty() && !only.contains(b.name.as_str()) {
            continue;
        }
        let visible = s.body_visible(&b.name);
        if !visible && !include_hidden {
            continue;
        }
        let settings = per_body.get(&b.name).unwrap_or(&base);
        let (_, matrix) = b.definition_in_place();
        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let color = s.doc.body_color(b).map(|c| c.map(to8)).unwrap_or([176, 186, 198]);
        let faces = s
            .doc
            .face_colors(b)
            .into_iter()
            .filter_map(|(face, look)| {
                Some(LiveFaceLook { face: u32::try_from(face).ok()?, color: look.color, opacity: look.opacity.clamp(0.0, 1.0) })
            })
            .collect();
        let mut object = LiveObject {
            key: b.name.clone(),
            definition: b.definition_name().to_string(),
            component: b.component,
            matrix: std::array::from_fn(|i| matrix.get(i / 4).and_then(|c| c.get(i % 4)).copied().unwrap_or(0.0)),
            mesh: None,
            visible,
            color,
            opacity: s.doc.model_body_opacity(b).clamp(0.0, 1.0),
            faces,
            fallback: false,
            error: None,
        };
        match mesh_of(b, settings) {
            Ok((mesh, fallback)) => {
                object.fallback = fallback;
                object.mesh = Some(mesh.key.clone());
                if !have.contains(mesh.key.as_str()) && !sent.contains(&mesh.key) {
                    // Base64 makes four characters of every three bytes.
                    let cost = mesh.bytes.div_ceil(3) * 4;
                    if meshes.is_empty() || size.saturating_add(cost) <= budget {
                        size = size.saturating_add(cost);
                        meshes.push(mesh.json.clone());
                    } else {
                        pending.push(mesh.key.clone());
                    }
                    sent.insert(mesh.key.clone());
                }
            }
            Err(e) => object.error = Some(e),
        }
        objects.push(object);
    }
    let components = s.doc.components.iter().map(|c| LiveComponent { id: c.id, name: c.name.clone(), parent: c.parent }).collect();
    let reply = LiveMeshesResult {
        document_id: s.document_id.clone(),
        unit: "mm".into(),
        up: "Z".into(),
        components,
        objects,
        meshes,
        pending,
        param_warnings: Vec::new(),
    };
    serde_json::to_value(reply).map_err(|e| EngineError::Other(e.to_string()))
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
