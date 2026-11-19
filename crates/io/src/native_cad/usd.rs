//! OpenUSD (`.usd`, `.usda`, `.usdc`, `.usdz`) through cadmpeg's USD codec, both ways.
//!
//! Export: the bodies are written as STEP by SolidCraft's own writer (an assembly as a STEP
//! assembly, as `file.export` writes it), cadmpeg reads that STEP into its neutral model, and
//! cadmpeg's USD writer writes the model: each part as an `Xform` prim holding its exact B-rep (a
//! `BrepArray` prim) and a `Mesh` of it, a part placed more than once written once under the
//! abstract `Prototypes` prim and referenced at each placement, and colours as `UsdPreviewSurface`
//! materials. The layer is in metres with Z up and says so. SolidCraft's colours are sRGB, and
//! USD's are linear, so the colours are converted on the way.
//!
//! Import: cadmpeg reads the layer (text, binary, or the root layer of a package) and composes it
//! within itself. Each mesh becomes a mesh body, and the prim tree becomes components, as a glTF
//! file's node tree does ([`crate::GltfImport`]): a prim placed rigidly is a component, a prim
//! that scales or mirrors is baked into its meshes, and a prim placed again with the same contents
//! is another occurrence of the same component. A material's base colour, opacity, metallic and
//! roughness, or else a constant display colour, give the body's look. What has no mesh (NURBS
//! patches, a B-rep with no mesh, curves, intrinsic shapes such as `Cube`) and references to other
//! files are left out with a warning. The file is untrusted: cadmpeg's service caps bound the
//! decode, and the vertices, triangles, placements, nesting and components read here are capped.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use cadmpeg_core::decode::DecodeDepth;
use cadmpeg_ir::appearance::AppearanceTarget;
use cadmpeg_ir::codec::write::{EncodeInput, target::TargetRequest};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, ProductDefinition, PrototypeReference};
use cadmpeg_ir::tessellation::Tessellation;
use cadmpeg_ir::topology::Color;
use cadmpeg_registry::{Format, build_encoder};
use solidcore_record::Vec3;
use solidcraft_doc::MeshData;
use solidcraft_doc::appearance::Look;

use super::convert::decode_at;
use crate::{GltfImport, MeshFeature, MeshPlan, PlanPlace};

/// File extensions of OpenUSD layers and packages (lower case).
pub const USD_EXTS: [&str; 4] = ["usd", "usda", "usdc", "usdz"];

/// cadmpeg's id for its USD codec.
const CODEC: &str = "usd";

/// Most vertices and triangles read, over all meshes (as for glTF).
const MAX_VERTICES: usize = 20_000_000;
const MAX_TRIANGLES: usize = 20_000_000;
/// Most prim placements followed.
const MAX_PLACEMENTS: usize = 100_000;
/// Deepest nesting followed.
const MAX_DEPTH: usize = 64;
/// Deepest nesting kept as components; deeper prims are baked into their parent's.
const MAX_COMPONENT_DEPTH: usize = 32;
/// Most components one file makes.
const MAX_COMPONENTS: usize = 5_000;

/// Losses cadmpeg reports that say nothing to the person exporting: how SolidCraft's own STEP
/// text was read on the way, and that mesh points are single precision, which every USD mesh is
/// (the exact B-rep beside it keeps double precision).
const QUIET_LOSSES: [&str; 4] =
    ["step/decode.opaque-record-preserved", "step/topology.pcurve-global-fidelity-unproved", "step/decode.warning", "usd/export.vertex-precision"];

/// Is this an OpenUSD file name (any case)?
pub fn is_usd_path(path: &str) -> bool {
    std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| USD_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// The cadmpeg write target a file name asks for: `.usda` text, `.usdz` a package, and `.usdc` or
/// `.usd` binary (a `.usd` layer is binary unless it says otherwise, as OpenUSD writes it).
fn target(file: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(file).extension().and_then(|e| e.to_str())?.to_ascii_lowercase();
    match ext.as_str() {
        "usda" => Some("usda"),
        "usdc" | "usd" => Some("usdc"),
        "usdz" => Some("usdz"),
        _ => None,
    }
}

/// `step` (STEP text SolidCraft wrote) as an OpenUSD file called `file`, whose extension picks the
/// encoding, with what the file could not carry.
pub(super) fn from_step(step: &[u8], file: &str) -> Result<(Vec<u8>, Vec<String>), String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    let target = target(&name).ok_or_else(|| format!("{name}: not an OpenUSD file name (.usda, .usdc, .usd or .usdz)"))?;
    let (_, decoded) = decode_at(&name, "step", step, &["step"], DecodeDepth::Full)?;
    let (mut ir, report, fidelity) = decoded.into_parts();
    if !report.geometry_transferred() {
        return Err(format!("{name}: there is no geometry to write"));
    }
    let stem = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    name_roots(&mut ir, &stem);
    colours_to_linear(&mut ir);
    let plan = build_encoder(Format::Usd)
        .plan(EncodeInput::new(&ir, Some(&fidelity)), TargetRequest::Explicit(target))
        .map_err(|e| format!("{name}: could not write OpenUSD: {e}"))?;
    let mut warnings: Vec<String> = Vec::new();
    for l in report.losses.iter().chain(&plan.report().losses) {
        let code = l.code.to_string();
        if !QUIET_LOSSES.contains(&code.as_str()) {
            warnings.push(format!("{code}: {}", l.message));
        }
    }
    let mut out = Vec::new();
    plan.write_to(&mut out).map_err(|e| format!("{name}: could not write OpenUSD: {e}"))?;
    Ok((out, warnings))
}

/// Name each root occurrence with no name after its part, else after the file, so the layer's
/// root prim is not called `Occurrence`.
fn name_roots(ir: &mut CadIr, stem: &str) {
    let labels: HashMap<String, String> =
        ir.model.product_definitions.iter().filter_map(|d| Some((d.id.as_str().to_string(), product_name(d)?))).collect();
    for o in ir.model.occurrences.iter_mut().filter(|o| matches!(o.parent, OccurrenceParent::Root {})) {
        if o.name.as_deref().is_some_and(|n| !n.trim().is_empty()) {
            continue;
        }
        let part = match &o.prototype {
            PrototypeReference::Local { definition } => labels.get(definition.as_str()).cloned(),
            _ => None,
        };
        o.name = part.or_else(|| (!stem.trim().is_empty()).then(|| stem.to_string()));
    }
}

/// SolidCraft's (and STEP's) colours are sRGB; USD's `displayColor` and `diffuseColor` are linear.
fn colours_to_linear(ir: &mut CadIr) {
    let linear = |c: Color| {
        let f = |v: f32| solidcraft_color::srgb::decode(f64::from(v)).clamp(0.0, 1.0) as f32;
        Color::new(f(c.r()), f(c.g()), f(c.b()), c.a()).unwrap_or(c)
    };
    for b in &mut ir.model.bodies {
        b.color = b.color.map(linear);
    }
    for f in &mut ir.model.faces {
        f.color = f.color.map(linear);
    }
    for a in &mut ir.model.appearances {
        a.base_color = a.base_color.map(linear);
    }
}

/// A definition's name: its label, else its name in the file, else its part number.
fn product_name(d: &ProductDefinition) -> Option<String> {
    [&d.label, &d.source_name, &d.part_number].into_iter().flatten().find(|n| !n.trim().is_empty()).map(|n| clean_name(n))
}

fn clean_name(s: &str) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(128).collect()
}

/// An OpenUSD file (`file` names it) read as mesh bodies and a component plan, and whether the
/// file says its Y axis is up (OpenUSD's default when it says nothing).
pub(super) fn read(bytes: &[u8], file: &str) -> Result<(GltfImport, bool), String> {
    let name = std::path::Path::new(file).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| file.to_string());
    crate::limits::check(crate::limits::Kind::Step, bytes.len() as u64).map_err(|e| format!("{name}: {e}"))?;
    let ext = std::path::Path::new(&name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    // cadmpeg checks each layer's encoding from its first bytes; the extension only decides when
    // they say nothing.
    let ext = if ext == "usd" { String::from("usdc") } else { ext };
    let (_, decoded) = decode_at(&name, &ext, bytes, &[CODEC], DecodeDepth::Full)?;
    let (ir, report, _) = decoded.into_parts();
    let y_up = ir
        .source
        .as_ref()
        .and_then(|s| s.attributes.iter().find(|(k, _)| k.as_str() == "up_axis").map(|(_, v)| v.clone()))
        .is_none_or(|axis| axis != "Z");
    let stem = std::path::Path::new(&name)
        .file_stem()
        .map(|s| clean_name(&s.to_string_lossy()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "OpenUSD".into());
    let mut r = Reader::new(&ir, stem.clone());
    r.walk_all();
    let mut warnings: Vec<String> = report.losses.iter().map(|l| format!("{}: {}", l.code, l.message)).collect();
    warnings.append(&mut r.warnings);
    if r.meshes.is_empty() {
        let why = warnings.first().map(|w| format!(" ({w})")).unwrap_or_default();
        return Err(format!("{name}: this OpenUSD file has no meshes that can be imported{why}"));
    }
    let kind = solidcraft_doc::FeatureKind::MeshImport { file: name.clone(), meshes: r.meshes.clone() };
    let feature_name = match r.meshes.as_slice() {
        [only] if !only.name.trim().is_empty() => only.name.clone(),
        _ => stem,
    };
    let feature = MeshFeature { name: feature_name, body_names: r.meshes.iter().map(|m| m.name.clone()).collect(), kind, warnings };
    Ok((GltfImport { feature, plan: r.plan, looks: r.body_looks }, y_up))
}

type M4 = [[f64; 4]; 4];

const IDENTITY: M4 = solidcraft_doc::IDENTITY;

/// Occurrence `o`'s whole placement in its parent (scale included), column-major, when finite.
fn placement(o: &Occurrence) -> Option<M4> {
    let t = o.effective_transform().ok()?;
    let r = t.rows();
    let mut m = IDENTITY;
    for (c, col) in m.iter_mut().enumerate() {
        for (row, v) in col.iter_mut().enumerate() {
            *v = r.get(row).and_then(|x| x.get(c)).copied().unwrap_or(0.0);
        }
    }
    let s = o.scale.map(|v| v.get());
    let m = solidcraft_doc::mat_mul(&m, &[[s[0], 0.0, 0.0, 0.0], [0.0, s[1], 0.0, 0.0], [0.0, 0.0, s[2], 0.0], [0.0, 0.0, 0.0, 1.0]]);
    m.iter().flatten().all(|v| v.is_finite()).then_some(m)
}

/// Whether `m` places without scaling or mirroring (to within rounding).
fn is_rigid(m: &M4) -> bool {
    let col = |c: usize| [m[c][0], m[c][1], m[c][2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (x, y, z) = (col(0), col(1), col(2));
    let unit = |v: [f64; 3]| (dot(v, v) - 1.0).abs() < 1e-9;
    unit(x) && unit(y) && unit(z) && dot(x, y).abs() < 1e-9 && dot(y, z).abs() < 1e-9 && dot(z, x).abs() < 1e-9 && det3(m) > 0.0
}

fn det3(m: &M4) -> f64 {
    let a = |r: usize, c: usize| m[c][r];
    a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1)) - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0))
}

fn hash_mat(m: &M4, h: &mut impl Hasher) {
    for v in m.iter().flatten() {
        // Equal to within rounding is equal.
        ((v * 1e6).round() as i64).hash(h);
    }
}

/// sRGB bytes of a linear USD colour.
fn srgb(c: Color) -> [u8; 3] {
    [c.r(), c.g(), c.b()].map(|v| solidcraft_color::srgb::encode8(f64::from(v)))
}

struct Reader<'a> {
    ir: &'a CadIr,
    definitions: HashMap<&'a str, &'a ProductDefinition>,
    children: HashMap<&'a str, Vec<&'a Occurrence>>,
    roots: Vec<&'a Occurrence>,
    tessellations: HashMap<&'a str, Vec<&'a Tessellation>>,
    /// The look of each body (by CADIR body id), from its material or its display colour.
    looks: HashMap<&'a str, Look>,
    meshes: Vec<MeshData>,
    body_looks: Vec<Option<Look>>,
    plan: MeshPlan,
    /// Components made, by what makes two placements the same component.
    keys: HashMap<u64, usize>,
    /// The components being filled (the current prim's ancestors).
    open: Vec<usize>,
    placements: usize,
    vertices: usize,
    triangles: usize,
    cut_off: bool,
    /// Bodies with exact surfaces but no mesh, left out.
    no_mesh: usize,
    hidden: usize,
    stem: String,
    warnings: Vec<String>,
    /// The bodies reached through some occurrence.
    reached: HashSet<&'a str>,
}

impl<'a> Reader<'a> {
    fn new(ir: &'a CadIr, stem: String) -> Self {
        let mut children: HashMap<&str, Vec<&Occurrence>> = HashMap::new();
        let mut roots = Vec::new();
        for o in &ir.model.occurrences {
            match &o.parent {
                OccurrenceParent::Root {} => roots.push(o),
                OccurrenceParent::Occurrence { occurrence } => children.entry(occurrence.as_str()).or_default().push(o),
            }
        }
        let order = |a: &&Occurrence, b: &&Occurrence| a.ordinal.cmp(&b.ordinal).then_with(|| a.id.as_str().cmp(b.id.as_str()));
        roots.sort_by(order);
        for list in children.values_mut() {
            list.sort_by(order);
        }
        let mut tessellations: HashMap<&str, Vec<&Tessellation>> = HashMap::new();
        for t in &ir.model.tessellations {
            if let Some(b) = &t.body {
                tessellations.entry(b.as_str()).or_default().push(t);
            }
        }
        let mut r = Reader {
            ir,
            definitions: ir.model.product_definitions.iter().map(|d| (d.id.as_str(), d)).collect(),
            children,
            roots,
            tessellations,
            looks: HashMap::new(),
            meshes: Vec::new(),
            body_looks: Vec::new(),
            plan: MeshPlan::default(),
            keys: HashMap::new(),
            open: Vec::new(),
            placements: 0,
            vertices: 0,
            triangles: 0,
            cut_off: false,
            no_mesh: 0,
            hidden: 0,
            stem,
            warnings: Vec::new(),
            reached: HashSet::new(),
        };
        r.read_looks();
        r
    }

    /// Each body's look: the material bound to the body or to its mesh, else its display colour.
    fn read_looks(&mut self) {
        let ir = self.ir;
        let appearances: HashMap<&str, &cadmpeg_ir::appearance::Appearance> = ir.model.appearances.iter().map(|a| (a.id.as_str(), a)).collect();
        let body_of_tessellation: HashMap<&str, &str> =
            ir.model.tessellations.iter().filter_map(|t| Some((t.id.as_str(), t.body.as_ref()?.as_str()))).collect();
        for b in &ir.model.bodies {
            if let Some(c) = b.color {
                self.looks.insert(b.id.as_str(), Look::imported(srgb(c), f64::from(c.a()), None));
            }
        }
        for binding in &ir.model.appearance_bindings {
            let body = match &binding.target {
                AppearanceTarget::Body(b) => Some(b.as_str()),
                AppearanceTarget::Tessellation(t) => body_of_tessellation.get(t.as_str()).copied(),
                _ => None,
            };
            let (Some(body), Some(a)) = (body, appearances.get(binding.appearance.as_str())) else { continue };
            let Some(c) = a.base_color else { continue };
            let opacity = f64::from(c.a());
            let mut look = Look::imported(srgb(c), opacity, a.name.as_deref().map(clean_name).filter(|n| !n.is_empty()).as_deref());
            let scalar = |k: &str| a.properties.iter().find(|(key, _)| key.as_str() == k).map(|(_, v)| v.get().clamp(0.0, 1.0));
            if opacity >= 1.0
                && let (Some(metallic), Some(roughness)) = (scalar("metallic").or(Some(0.0)), scalar("roughness").or(Some(0.5)))
                && (scalar("metallic").is_some() || scalar("roughness").is_some())
            {
                look.pbr = solidcraft_doc::pbr_appearance::PbrParameters::new(metallic, roughness, 1.5).ok();
            }
            self.looks.insert(body, look);
        }
    }

    fn walk_all(&mut self) {
        let roots = self.roots.clone();
        for o in roots {
            self.walk(o, None, None, 0);
        }
        // Bodies no prim placement reaches stay where the file defines them.
        let ir = self.ir;
        for d in &ir.model.product_definitions {
            for b in &d.bodies {
                if !self.reached.contains(b.as_str()) {
                    self.body_meshes(b.as_str(), &product_name(d).unwrap_or_else(|| self.stem.clone()), None, &IDENTITY);
                }
            }
        }
        if self.cut_off {
            self.warnings.push("the prim tree is too large or too deep: the rest was left out".into());
        }
        if self.no_mesh > 0 {
            self.warnings.push(format!(
                "{} bodies have exact surfaces but no mesh, and were left out: only meshes are imported from OpenUSD so far",
                self.no_mesh
            ));
        }
        if self.hidden > 0 {
            self.warnings.push(format!("{} invisible prims were left out", self.hidden));
        }
    }

    fn budget(&mut self, depth: usize) -> bool {
        self.placements += 1;
        if self.placements > MAX_PLACEMENTS || depth > MAX_DEPTH {
            self.cut_off = true;
            return false;
        }
        true
    }

    fn definition(&self, o: &Occurrence) -> Option<&'a ProductDefinition> {
        match &o.prototype {
            PrototypeReference::Local { definition } => self.definitions.get(definition.as_str()).copied(),
            _ => None,
        }
    }

    /// The name of occurrence `o`: its own, else its part's.
    fn name(&self, o: &Occurrence) -> String {
        o.name
            .as_deref()
            .map(clean_name)
            .filter(|n| !n.is_empty())
            .or_else(|| self.definition(o).and_then(product_name))
            .unwrap_or_else(|| format!("Prim {}", self.plan.slots.len() + 1))
    }

    /// Occurrence `o` under component `parent` (`None`: the import's own). With `bake`, `o` and
    /// everything below it are built in `parent`'s frame, `bake` being the frame `o`'s parent
    /// prim has there.
    fn walk(&mut self, o: &'a Occurrence, parent: Option<usize>, bake: Option<&M4>, depth: usize) {
        if !self.budget(depth) {
            return;
        }
        let label = self.name(o);
        if let Some(d) = self.definition(o) {
            self.reached.extend(d.bodies.iter().map(|b| b.as_str()));
        }
        if o.visible == Some(false) {
            self.hidden += 1;
            return;
        }
        let Some(local) = placement(o) else {
            self.warnings.push(format!("{label}: not imported: its placement is not finite"));
            return;
        };
        if matches!(o.prototype, PrototypeReference::External { .. }) {
            self.warnings.push(format!("{label}: not imported: it is a reference to another file"));
            return;
        }
        let definition = self.definition(o);
        let kids = self.children.get(o.id.as_str()).cloned().unwrap_or_default();
        let component = bake.is_none() && is_rigid(&local) && depth < MAX_COMPONENT_DEPTH && self.plan.slots.len() < MAX_COMPONENTS;
        if !component {
            let frame = solidcraft_doc::mat_mul(bake.unwrap_or(&IDENTITY), &local);
            if let Some(d) = definition {
                for b in &d.bodies {
                    self.body_meshes(b.as_str(), &label, parent, &frame);
                }
            }
            for c in kids {
                self.walk(c, parent, Some(&frame), depth + 1);
            }
            return;
        }
        let key = self.key(o, 0);
        if let Some(&slot) = self.keys.get(&key) {
            if self.open.contains(&slot) {
                // A prim inside itself: not followed.
                self.cut_off = true;
                return;
            }
            self.plan.places.push(PlanPlace { slot, parent, transform: local });
            return;
        }
        let slot = self.plan.slots.len();
        self.plan.slots.push(label.clone());
        self.plan.places.push(PlanPlace { slot, parent, transform: local });
        self.keys.insert(key, slot);
        if let Some(d) = definition {
            for b in &d.bodies {
                self.body_meshes(b.as_str(), &label, Some(slot), &IDENTITY);
            }
        }
        self.open.push(slot);
        for c in kids {
            self.walk(c, Some(slot), None, depth + 1);
        }
        self.open.pop();
    }

    /// What makes two placements the same component: the part, and the same prims placed the
    /// same way below it.
    fn key(&self, o: &Occurrence, depth: usize) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match &o.prototype {
            PrototypeReference::Local { definition } => definition.as_str().hash(&mut h),
            _ => o.id.as_str().hash(&mut h),
        }
        if depth < 8 {
            for c in self.children.get(o.id.as_str()).into_iter().flatten() {
                if let Some(m) = placement(c) {
                    hash_mat(&m, &mut h);
                }
                (c.visible == Some(false)).hash(&mut h);
                self.key(c, depth + 1).hash(&mut h);
            }
        } else {
            // Too deep to compare: only the very same prim is the same.
            o.id.as_str().hash(&mut h);
        }
        h.finish()
    }

    /// The meshes of CADIR body `body` as mesh bodies in component `slot`, placed there by `bake`.
    fn body_meshes(&mut self, body: &str, owner: &str, slot: Option<usize>, bake: &M4) {
        let ir = self.ir;
        let Some(b) = ir.model.bodies.iter().find(|b| b.id.as_str() == body) else { return };
        if b.visible == Some(false) {
            self.hidden += 1;
            return;
        }
        let tessellations = self.tessellations.get(body).cloned().unwrap_or_default();
        if tessellations.is_empty() {
            if !b.regions.is_empty() {
                self.no_mesh += 1;
            }
            return;
        }
        if det3(bake).abs() < 1e-18 {
            self.warnings.push(format!("{owner}: not imported: its placement scales it to nothing"));
            return;
        }
        let flip = det3(bake) < 0.0;
        // Normals follow a rigid placement; a scaled one is shaded flat.
        let turn_normals = is_rigid(bake) || *bake == IDENTITY;
        let base = b.name.as_deref().map(clean_name).filter(|n| !n.is_empty()).unwrap_or_else(|| owner.to_string());
        let several = tessellations.len() > 1;
        for (k, t) in tessellations.into_iter().enumerate() {
            let vertices = t.vertices();
            let triangles = t.triangles();
            if self.vertices + vertices.len() > MAX_VERTICES || self.triangles + triangles.len() > MAX_TRIANGLES {
                self.cut_off = true;
                return;
            }
            self.vertices += vertices.len();
            self.triangles += triangles.len();
            let name = if several { format!("{base} ({})", k + 1) } else { base.clone() };
            let positions: Vec<Vec3> = vertices
                .iter()
                .map(|p| {
                    let p = p.get();
                    solidcraft_doc::apply_point(bake, Vec3::new(p.x, p.y, p.z))
                })
                .collect();
            let triangles: Vec<[u32; 3]> = triangles.into_iter().map(|t| if flip { [t[0], t[2], t[1]] } else { t }).collect();
            if let Err(e) = solidcraft_kernel::mesh_body(&positions, &triangles) {
                self.warnings.push(format!("mesh `{name}`: not imported: {e}"));
                continue;
            }
            // Normals per triangle corner, or per vertex spread to each corner that uses it.
            let mut normals = t.per_corner_normals();
            if normals.is_empty() {
                let per_vertex = t.vertex_normals();
                if per_vertex.len() == vertices.len() {
                    normals = triangles.iter().flatten().filter_map(|&i| per_vertex.get(i as usize).copied()).collect();
                }
            }
            let corner_normals: Vec<f64> = if turn_normals && !flip && normals.len() == triangles.len() * 3 {
                normals
                    .iter()
                    .flat_map(|n| {
                        let n = n.get();
                        let v = solidcraft_doc::apply_vector(bake, Vec3::new(n.x, n.y, n.z));
                        [v.x, v.y, v.z]
                    })
                    .collect()
            } else {
                Vec::new()
            };
            self.meshes.push(MeshData {
                name,
                positions: positions.iter().flat_map(|p| [p.x, p.y, p.z]).collect(),
                triangles: triangles.iter().flatten().copied().collect(),
                corner_normals: corner_normals.into(),
                color: None,
            });
            self.body_looks.push(self.looks.get(body).cloned());
            self.plan.bodies.push(slot);
        }
    }
}

#[cfg(test)]
#[path = "usd_tests.rs"]
mod tests;
