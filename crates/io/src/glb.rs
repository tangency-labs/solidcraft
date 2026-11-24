//! GLB export (#1553): the bodies of a model as one binary glTF 2.0 file, for web viewers,
//! renderers and game engines. Each body is a node with its mesh, placed where the model has it;
//! its colours, metallic and roughness are glTF materials. One root node turns millimetres and
//! Z-up into the metres and Y-up glTF uses, so nothing is converted twice.
//!
//! An assembly keeps its shape (#5296): each occurrence a body is placed through is a node named
//! after it, placed in its parent as the occurrence is, and the body's node sits under it with its
//! mesh in the component's frame. A viewer can then pick and move a part on its own, turning it
//! about the component's origin. Occurrences of one part (a reused component, #19468) write its
//! mesh once and their body nodes all point at it, since the mesh is in the component's frame.
//! Without `hierarchy`, every body is a node of the root in world coordinates under its world
//! name, as SolidCraft 0.40 wrote it.
//!
//! A look that is not opaque is alpha blended (#5296). Glass a render setup gives a body is
//! written with glTF's transmission, IOR and volume extensions, and dispersion and a thin film
//! where it has them; Glass that scatters light inside it is refused, as glTF has no place for
//! it. Image textures are refused by the engine, which knows the render setups that bind them.

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_doc::render_optics::{OpticalBoundary, OpticalMaterial};
use solidcraft_doc::{Document, Mat, ModelState};
use solidcraft_geom::Mesh;

use crate::{IoError, MeshQuality, Result, display_mesh_at, selected};

/// Largest GLB file written: a quarter of what the format's 32-bit lengths allow, so a model
/// meshed far too finely is refused before its arrays are built.
pub const MAX_GLB_BYTES: usize = 1 << 30;

/// The colour of a body with no colour of its own (sRGB): the render workspace's.
pub const DEFAULT_COLOR: [u8; 3] = [176, 186, 198];

/// The metallic and roughness of a look without physical (PBR) parameters: those a new PBR look
/// starts with.
const PLAIN_METALLIC: f64 = 0.0;
const PLAIN_ROUGHNESS: f64 = 0.5;

/// The index of refraction glTF assumes when a material does not give one.
const DEFAULT_IOR: f64 = 1.5;

/// glTF's buffer view targets and component types.
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;

/// A glTF material: a look's name, sRGB colour, metallic, roughness, index of refraction and
/// opacity (`alpha`, 1 for an opaque look), or the Glass a render setup gives a body.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GlbMaterial {
    pub name: String,
    pub color: [u8; 3],
    pub metallic: f64,
    pub roughness: f64,
    pub ior: f64,
    pub alpha: f64,
    pub glass: Option<GlbGlass>,
}

/// Glass as glTF's transmission extensions write it. `tint` is the linear colour light keeps
/// passing through (the absorption of a thin sheet); `thickness` is 0 for a thin sheet, else the
/// body's thickness in its mesh's millimetres, with `attenuation` (a linear colour and the
/// distance in metres light travels to keep it) when the glass absorbs. `dispersion` is glTF's
/// (20 over the Abbe number) and `film` a thin film's IOR and thickness in nanometres.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GlbGlass {
    pub tint: [f64; 3],
    pub thickness: f64,
    pub attenuation: Option<([f64; 3], f64)>,
    pub dispersion: Option<f64>,
    pub film: Option<(f64, f64)>,
}

impl GlbMaterial {
    /// The material of a look, its opacity written as alpha blending.
    fn of_look(look: &solidcraft_doc::appearance::Look) -> GlbMaterial {
        let (metallic, roughness, ior) =
            look.pbr.map_or((PLAIN_METALLIC, PLAIN_ROUGHNESS, DEFAULT_IOR), |p| (p.metallic(), p.roughness(), p.dielectric_ior()));
        GlbMaterial { name: look.name.clone(), color: look.color, metallic, roughness, ior, alpha: unit(look.opacity), glass: None }
    }

    /// The glass of `optics` on a body named `name` whose mesh is `mesh`. Subsurface scattering
    /// is refused: glTF has no place for it, and the glass without it would look clear.
    fn of_glass(name: &str, optics: &OpticalMaterial, mesh: &Mesh) -> Result<GlbMaterial> {
        if optics.subsurface.is_some_and(|s| s.active()) {
            return Err(IoError::Invalid(format!(
                "{name}: its Glass scatters light inside it (subsurface scattering), which GLB cannot hold; turn the scattering off in the render setup, or leave the body out with `bodies`"
            )));
        }
        let absorb = optics.absorption_per_mm.map(|a| if a.is_finite() { a.max(0.0) } else { 0.0 });
        let model = optics.model;
        let thin = optics.boundary == OpticalBoundary::ThinSheet;
        let glass = if thin {
            // A thin sheet has no volume in glTF: the light its thickness absorbs tints it.
            let t = if optics.thickness_mm.is_finite() { optics.thickness_mm.max(0.0) } else { 0.0 };
            GlbGlass { tint: absorb.map(|a| (-a * t).exp()), thickness: 0.0, attenuation: None, dispersion: None, film: None }
        } else {
            // Beer-Lambert: light keeps exp(-a d) of each channel over d millimetres, so over
            // 1 / (strongest a) it keeps exp(-a / strongest), and glTF's distance is in metres.
            let strongest = absorb.iter().copied().fold(0.0, f64::max);
            let attenuation = (strongest > 0.0).then(|| (absorb.map(|a| (-a / strongest).exp()), 0.001 / strongest));
            let dispersion = model.and_then(|m| m.dispersion).map(|d| 20.0 / d.abbe_number).filter(|d| d.is_finite() && *d >= 0.0);
            GlbGlass { tint: [1.0; 3], thickness: thickness(mesh), attenuation, dispersion, film: None }
        };
        let film = model.and_then(|m| m.thin_film).map(|f| (f.ior, f.thickness_nm));
        Ok(GlbMaterial {
            name: name.to_string(),
            color: [255; 3],
            metallic: 0.0,
            roughness: model.map_or(0.0, |m| m.roughness),
            ior: optics.ior,
            alpha: 1.0,
            glass: Some(GlbGlass { film, ..glass }),
        })
    }

    /// The glTF material: linear base colour, metallic and roughness, alpha blending when the
    /// look is not opaque, and the extensions IOR and glass need.
    fn json(&self) -> Value {
        let [r, g, b] = match &self.glass {
            Some(glass) => glass.tint.map(unit),
            None => self.color.map(solidcraft_color::srgb::decode8),
        };
        let mut m = json!({
            "name": self.name,
            "pbrMetallicRoughness": {
                "baseColorFactor": [r, g, b, unit(self.alpha)],
                "metallicFactor": unit(self.metallic),
                "roughnessFactor": unit(self.roughness),
            },
        });
        if self.alpha < 1.0 {
            m["alphaMode"] = json!("BLEND");
        }
        let mut extensions = serde_json::Map::new();
        if self.writes_ior() {
            extensions.insert("KHR_materials_ior".into(), json!({"ior": self.ior}));
        }
        if let Some(glass) = &self.glass {
            extensions.insert("KHR_materials_transmission".into(), json!({"transmissionFactor": 1.0}));
            if glass.thickness.is_finite() && glass.thickness > 0.0 {
                let mut volume = json!({"thicknessFactor": glass.thickness});
                if let Some((color, distance)) = glass.attenuation.filter(|(_, d)| d.is_finite() && *d > 0.0) {
                    volume["attenuationColor"] = json!(color.map(unit));
                    volume["attenuationDistance"] = json!(distance);
                }
                extensions.insert("KHR_materials_volume".into(), volume);
                if let Some(d) = glass.dispersion.filter(|d| d.is_finite() && *d > 0.0) {
                    extensions.insert("KHR_materials_dispersion".into(), json!({"dispersion": d}));
                }
            }
            if let Some((ior, nm)) = glass.film.filter(|(i, t)| i.is_finite() && *i >= 1.0 && t.is_finite() && *t >= 0.0) {
                extensions.insert(
                    "KHR_materials_iridescence".into(),
                    // Without a thickness texture glTF reads the maximum alone.
                    json!({"iridescenceFactor": 1.0, "iridescenceIor": ior, "iridescenceThicknessMaximum": nm}),
                );
            }
        }
        if !extensions.is_empty() {
            m["extensions"] = Value::Object(extensions);
        }
        m
    }

    /// glTF's default IOR is left unwritten, and so is one the extension cannot hold.
    fn writes_ior(&self) -> bool {
        self.ior.is_finite() && self.ior >= 1.0 && self.ior != DEFAULT_IOR
    }
}

/// A closed body's thickness for glTF's volume: the smallest side of its mesh's bounding box (0
/// when it has no points), which a viewer without the geometry uses to darken light through it.
fn thickness(mesh: &Mesh) -> f64 {
    let b = solidcraft_geom::MeshGeom::bounds(mesh);
    let side = (b.max.x - b.min.x).min(b.max.y - b.min.y).min(b.max.z - b.min.z);
    if side.is_finite() { side.max(0.0) } else { 0.0 }
}

/// A fraction between 0 and 1 (0 when it is not a number).
fn unit(x: f64) -> f64 {
    if x.is_finite() { x.clamp(0.0, 1.0) } else { 0.0 }
}

/// A body to write: its name, its mesh in millimetres and Z-up, its own material and the
/// materials of faces that have one of their own (by B-rep face index). `parent` is the
/// occurrence node it sits under (an index into the [`GlbNode`]s), its mesh then in that
/// occurrence's component frame; `None` puts it under the root, its mesh in the world.
pub(crate) struct GlbBody {
    pub name: String,
    pub mesh: Mesh,
    pub material: usize,
    pub faces: BTreeMap<u32, usize>,
    pub parent: Option<usize>,
}

/// An occurrence's node: its name, the node it sits in (`None`: the root) and its placement there
/// (millimetres, column-major, as glTF's node `matrix` and the document's [`Mat`] both are).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GlbNode {
    pub name: String,
    pub parent: Option<usize>,
    pub matrix: Mat,
}

/// The occurrence nodes made so far, by occurrence path, each with its world transform.
#[derive(Default)]
struct Tree {
    nodes: Vec<GlbNode>,
    by_path: BTreeMap<Vec<u64>, (usize, Mat)>,
}

impl Tree {
    /// The node of the occurrence `path` leads to (making it and its parents' nodes when they are
    /// not made yet) and its world transform. `None` when the path no longer exists or an
    /// occurrence on it is not a rigid placement, so its bodies are written in the world.
    fn node(&mut self, doc: &Document, path: &[u64]) -> Option<(usize, Mat)> {
        if path.is_empty() || path.len() > solidcraft_doc::MAX_PLACEMENT_DEPTH + 1 {
            return None;
        }
        if let Some(found) = self.by_path.get(path) {
            return Some(*found);
        }
        let (parent, above, component) = match path.split_last() {
            Some((_, [])) => (None, solidcraft_doc::IDENTITY, 0),
            Some((_, before)) => {
                let (node, m) = self.node(doc, before)?;
                (Some(node), m, doc.path_placement(before)?.0)
            }
            None => return None,
        };
        let id = *path.last()?;
        let o = doc.occurrences.iter().find(|o| o.id == id && o.parent == component)?;
        if !solidcraft_doc::is_placement(&o.transform) {
            return None;
        }
        let m = solidcraft_doc::mat_mul(&above, &o.transform);
        self.nodes.push(GlbNode { name: o.name.clone(), parent, matrix: o.transform });
        let found = (self.nodes.len() - 1, m);
        self.by_path.insert(path.to_vec(), found);
        Some(found)
    }
}

/// `mesh` moved by `m` (an inverse placement: a rotation and a translation), its normals turned.
fn moved(mut mesh: Mesh, m: &Mat) -> Mesh {
    for p in &mut mesh.positions {
        *p = solidcraft_doc::apply_point(m, *p);
    }
    for n in &mut mesh.normals {
        *n = solidcraft_doc::apply_vector(m, *n);
    }
    mesh
}

/// How a GLB is written: the mesh quality, whether an assembly keeps its occurrence nodes, and
/// the Glass a render setup gives bodies, by world name.
#[derive(Clone, Debug)]
pub struct GlbOptions {
    pub quality: MeshQuality,
    pub hierarchy: bool,
    pub glass: BTreeMap<String, OpticalMaterial>,
}

impl GlbOptions {
    /// Options with no Glass.
    pub fn new(quality: MeshQuality, hierarchy: bool) -> GlbOptions {
        GlbOptions { quality, hierarchy, glass: BTreeMap::new() }
    }
}

/// The GLB of the given bodies of `world`, the model placed in the world (all when `bodies` is
/// empty): each with the colour, metallic, roughness and opacity the design shows it with, as
/// the render workspace resolves them (a face's look over its body's, a body's over its
/// component's and material's, else the colour it was imported with). A look that is not opaque
/// is alpha blended; a body `opts` gives Glass is glass on every face. `model` is the model
/// before placing, which says the occurrence each placed body comes through: those bodies sit
/// under their occurrences' nodes; `doc`'s occurrences must place them as `world` has them.
/// Without `opts.hierarchy` every body is written in the world under the root, by its world
/// name. `name` names the scene.
pub fn export_glb(doc: &Document, model: &ModelState, world: &ModelState, bodies: &[String], name: &str, opts: &GlbOptions) -> Result<Vec<u8>> {
    let _span = solidcraft_profile::span("io", "export_glb");
    let sel = selected(world, bodies)?;
    // Each placed body's occurrence path, by its world name.
    let paths: BTreeMap<String, Vec<u64>> = if !opts.hierarchy || doc.occurrences.is_empty() {
        BTreeMap::new()
    } else {
        doc.body_frames(model).into_iter().map(|(n, _, path, _)| (n, path)).collect()
    };
    let mut tree = Tree::default();
    let mut materials: Vec<GlbMaterial> = Vec::new();
    // Two materials are one when glTF writes them the same.
    let mut index: BTreeMap<String, usize> = BTreeMap::new();
    let mut add = |m: GlbMaterial| {
        *index.entry(m.json().to_string()).or_insert_with(|| {
            materials.push(m);
            materials.len() - 1
        })
    };
    let mut out = Vec::with_capacity(sel.len());
    let mut total = 0;
    for b in sel {
        let mesh = display_mesh_at(&b.body, opts.quality)?;
        // Under its occurrence, in the component's frame, named as the component names it; a body
        // not placed through an occurrence stays in the world under its world name.
        let placed =
            paths.get(&b.name).and_then(|path| tree.node(doc, path)).and_then(|(node, m)| solidcraft_doc::mat_inverse(&m).map(|inv| (node, inv)));
        let (body_name, mesh, parent) = match placed {
            Some((node, inv)) => (b.definition_name().to_string(), moved(mesh, &inv), Some(node)),
            None => (b.name.clone(), mesh, None),
        };
        let mut faces = BTreeMap::new();
        let material = match opts.glass.get(&b.name) {
            Some(optics) => {
                // Named as its part names the body, as the plain material is below.
                let mut glass = GlbMaterial::of_glass(&b.name, optics, &mesh)?;
                glass.name = b.definition_name().to_string();
                add(glass)
            }
            None => {
                let material = match doc.model_body_look(b) {
                    Some(look) => GlbMaterial::of_look(&look),
                    None => {
                        // Named as its part names the body, so the placements of a part share one
                        // material (and so one mesh, #19468).
                        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                        let color = doc.body_color(b).map_or(DEFAULT_COLOR, |c| c.map(to8));
                        let alpha = unit(doc.model_body_opacity(b));
                        GlbMaterial {
                            name: b.definition_name().to_string(),
                            color,
                            metallic: PLAIN_METALLIC,
                            roughness: PLAIN_ROUGHNESS,
                            ior: DEFAULT_IOR,
                            alpha,
                            glass: None,
                        }
                    }
                };
                let material = add(material);
                for (face, look) in doc.face_colors(b) {
                    let face = u32::try_from(face).map_err(|_| IoError::Invalid(format!("{}: face {face} is out of range for GLB", b.name)))?;
                    faces.insert(face, add(GlbMaterial::of_look(&look)));
                }
                material
            }
        };
        let body = GlbBody { name: body_name, mesh, material, faces, parent };
        // Refused as soon as the meshes kept so far are too large, not after meshing them all.
        total = add_to_budget(total, &body)?;
        out.push(body);
    }
    glb(name, &tree.nodes, &out, &materials)
}

/// A body's arrays as they go in the binary chunk, checked: positions (mm, f32), unit normals
/// (left out when the mesh has none, or one is not a direction), and its triangles grouped by
/// material.
struct Arrays {
    positions: Vec<[f32; 3]>,
    min: [f32; 3],
    max: [f32; 3],
    normals: Option<Vec<[f32; 3]>>,
    groups: BTreeMap<usize, Vec<[u32; 3]>>,
}

/// `b`'s mesh as [`Arrays`], refused when a point is not finite, a triangle uses a point the mesh
/// does not have, or a material index is not one of the `materials`.
fn arrays(b: &GlbBody, materials: usize) -> Result<Arrays> {
    let m = &b.mesh;
    let count = u32::try_from(m.positions.len()).ok().filter(|n| *n < u32::MAX).ok_or_else(|| too_large(&b.name))?;
    let mut positions = Vec::new();
    positions.try_reserve_exact(m.positions.len()).map_err(|_| too_large(&b.name))?;
    let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
    for p in &m.positions {
        let v = [p.x as f32, p.y as f32, p.z as f32];
        if !v.iter().all(|x| x.is_finite()) {
            return Err(IoError::Invalid(format!("{}: the mesh has a point that is not a finite number", b.name)));
        }
        for i in 0..3 {
            min[i] = min[i].min(v[i]);
            max[i] = max[i].max(v[i]);
        }
        positions.push(v);
    }
    let normals = (m.normals.len() == m.positions.len())
        .then(|| {
            m.normals
                .iter()
                .map(|n| {
                    let v = [n.x as f32, n.y as f32, n.z as f32];
                    let len = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                    (len.is_finite() && len > 0.0).then(|| v.map(|x| x / len))
                })
                .collect::<Option<Vec<_>>>()
        })
        .flatten();
    let check =
        |i: usize| -> Result<usize> { if i < materials { Ok(i) } else { Err(IoError::Invalid(format!("{}: material {i} does not exist", b.name))) } };
    let base = check(b.material)?;
    let mut groups: BTreeMap<usize, Vec<[u32; 3]>> = BTreeMap::new();
    for (i, t) in m.triangles.iter().enumerate() {
        if t.iter().any(|v| *v >= count) {
            return Err(IoError::Invalid(format!("{}: triangle {i} uses a point the mesh does not have", b.name)));
        }
        // A triangle with a repeated corner has no area and nothing to draw.
        if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
            continue;
        }
        let face = m.tri_face.get(i).copied();
        let material = match face.and_then(|f| b.faces.get(&f)) {
            Some(&f) => check(f)?,
            None => base,
        };
        groups.entry(material).or_default().push(*t);
    }
    Ok(Arrays { positions, min, max, normals, groups })
}

impl Arrays {
    /// Whether `other` writes the same mesh: the same triangles per material and the same points
    /// and normals, to within the rounding a placement's inverse leaves behind (a quarter turn
    /// leaves 1e-16 where the other occurrence has 0), far below what an f32 file can show.
    fn same_as(&self, other: &Arrays) -> bool {
        let close = |a: &[[f32; 3]], b: &[[f32; 3]], abs: f32, rel: f32| {
            a.len() == b.len() && a.iter().flatten().zip(b.iter().flatten()).all(|(x, y)| (x - y).abs() <= abs + rel * x.abs().max(y.abs()))
        };
        let normals = match (&self.normals, &other.normals) {
            (Some(a), Some(b)) => close(a, b, 1e-5, 0.0),
            (None, None) => true,
            _ => false,
        };
        normals && self.groups == other.groups && close(&self.positions, &other.positions, 1e-6, 2e-6)
    }
}

/// A hash of what two bodies' meshes share when they are the same: name, point count, whether
/// there are normals, and the triangles per material (points and normals are compared after).
fn mesh_hash(name: &str, a: &Arrays) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::hash::DefaultHasher::new();
    name.hash(&mut h);
    a.positions.len().hash(&mut h);
    a.normals.is_some().hash(&mut h);
    for (material, tris) in &a.groups {
        material.hash(&mut h);
        tris.hash(&mut h);
    }
    h.finish()
}

/// Why `body` is not written: the file would pass [`MAX_GLB_BYTES`].
fn too_large(body: &str) -> IoError {
    IoError::Invalid(format!(
        "{body}: the mesh is too large for a GLB file (at most {} MiB); export fewer bodies or use standard quality",
        MAX_GLB_BYTES >> 20
    ))
}

/// The size of each body's arrays in the binary chunk, refused when the sum is larger than
/// [`MAX_GLB_BYTES`], before anything is built.
fn check_budget(bodies: &[GlbBody]) -> Result<()> {
    bodies.iter().try_fold(0, add_to_budget).map(|_| ())
}

/// `total` with `b`'s arrays added, refused when that is larger than [`MAX_GLB_BYTES`].
fn add_to_budget(total: usize, b: &GlbBody) -> Result<usize> {
    let vertices = b.mesh.positions.len().checked_mul(24);
    let triangles = b.mesh.triangles.len().checked_mul(12);
    vertices
        .zip(triangles)
        .and_then(|(v, t)| v.checked_add(t))
        .and_then(|n| total.checked_add(n))
        .filter(|n| *n <= MAX_GLB_BYTES)
        .ok_or_else(|| too_large(&b.name))
}

/// The binary chunk being built, with the glTF buffer views and accessors that describe it.
#[derive(Default)]
struct Chunk {
    bin: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Chunk {
    /// Append `data` as a buffer view for `target` and return the view's index.
    fn view(&mut self, data: impl Iterator<Item = [u8; 4]>, target: u32) -> Result<usize> {
        let offset = self.bin.len();
        for d in data {
            self.bin.extend_from_slice(&d);
        }
        let length = self.bin.len() - offset;
        if self.bin.len() > MAX_GLB_BYTES {
            return Err(IoError::Invalid(format!("the model is too large for a GLB file (at most {} MiB)", MAX_GLB_BYTES >> 20)));
        }
        self.views.push(json!({"buffer": 0, "byteOffset": offset, "byteLength": length, "target": target}));
        Ok(self.views.len() - 1)
    }

    /// Add accessor `a` and return its index.
    fn accessor(&mut self, a: Value) -> usize {
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    /// Append `v` as a vertex-attribute buffer view and return the view's index.
    fn vec3s(&mut self, v: &[[f32; 3]]) -> Result<usize> {
        self.view(v.iter().flatten().map(|x| x.to_le_bytes()), ARRAY_BUFFER)
    }
}

/// The GLB of `bodies` with `materials` under the occurrence nodes `tree`, its scene named `name`.
/// The root is node 0, the occurrences follow in order, then the bodies.
pub(crate) fn glb(name: &str, tree: &[GlbNode], bodies: &[GlbBody], materials: &[GlbMaterial]) -> Result<Vec<u8>> {
    check_budget(bodies)?;
    let mut chunk = Chunk::default();
    // Each node's children by the node they sit in: 0 the root, 1 + i occurrence i.
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); tree.len() + 1];
    let slot = |parent: Option<usize>| -> Result<usize> {
        match parent {
            None => Ok(0),
            Some(p) if p < tree.len() => Ok(p + 1),
            Some(p) => Err(IoError::Invalid(format!("occurrence node {p} does not exist"))),
        }
    };
    let mut nodes: Vec<Value> = Vec::with_capacity(tree.len() + bodies.len() + 1);
    for (i, n) in tree.iter().enumerate() {
        let at = slot(n.parent)?;
        // Parents come first, so the tree has no loop.
        if at > i {
            return Err(IoError::Invalid(format!("occurrence node {i} sits in a later one")));
        }
        children[at].push(i + 1);
        nodes.push(json!({"name": n.name, "matrix": n.matrix.iter().flatten().collect::<Vec<_>>()}));
    }
    let mut meshes = Vec::new();
    // Meshes written so far, by hash, as (name, arrays, mesh index): a body whose name, points,
    // normals and triangles per material are exactly those of one written (a reused component's
    // other occurrences) points its node at that mesh instead of writing it again.
    let mut written: std::collections::HashMap<u64, Vec<(&str, Arrays, usize)>> = std::collections::HashMap::new();
    for b in bodies {
        let a = arrays(b, materials.len())?;
        if a.groups.is_empty() {
            continue;
        }
        let hash = mesh_hash(&b.name, &a);
        if let Some((_, _, mesh)) = written.get(&hash).and_then(|c| c.iter().find(|(n, w, _)| *n == b.name && w.same_as(&a))) {
            nodes.push(json!({"name": b.name, "mesh": mesh}));
            children[slot(b.parent)?].push(nodes.len());
            continue;
        }
        let view = chunk.vec3s(&a.positions)?;
        let position = chunk.accessor(json!({
            "bufferView": view, "componentType": FLOAT, "count": a.positions.len(), "type": "VEC3",
            "min": a.min.map(f64::from), "max": a.max.map(f64::from),
        }));
        let normal = match &a.normals {
            Some(n) => {
                let view = chunk.vec3s(n)?;
                Some(chunk.accessor(json!({"bufferView": view, "componentType": FLOAT, "count": n.len(), "type": "VEC3"})))
            }
            None => None,
        };
        let mut primitives = Vec::new();
        for (material, tris) in &a.groups {
            let view = chunk.view(tris.iter().flatten().map(|i| i.to_le_bytes()), ELEMENT_ARRAY_BUFFER)?;
            let count = tris.len().checked_mul(3).ok_or_else(|| too_large(&b.name))?;
            let indices = chunk.accessor(json!({"bufferView": view, "componentType": UNSIGNED_INT, "count": count, "type": "SCALAR"}));
            let mut attributes = json!({"POSITION": position});
            if let Some(n) = normal {
                attributes["NORMAL"] = json!(n);
            }
            primitives.push(json!({"attributes": attributes, "indices": indices, "material": material}));
        }
        meshes.push(json!({"name": b.name, "primitives": primitives}));
        written.entry(hash).or_default().push((&b.name, a, meshes.len() - 1));
        nodes.push(json!({"name": b.name, "mesh": meshes.len() - 1}));
        children[slot(b.parent)?].push(nodes.len());
    }
    if meshes.is_empty() {
        return Err(IoError::Empty);
    }
    // glTF lists children only when there are some.
    for (node, kids) in nodes.iter_mut().zip(&children[1..]) {
        if !kids.is_empty() {
            node["children"] = json!(kids);
        }
    }
    // The root: millimetres to metres, and Z up to Y up (a quarter turn back about X), so
    // (x, y, z) in the model is 0.001 (x, z, -y) in the file.
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let root = json!({"name": name, "children": children[0], "rotation": [-half, 0.0, 0.0, half], "scale": [0.001, 0.001, 0.001]});
    nodes.insert(0, root);
    let mut gltf = json!({
        "asset": {"version": "2.0", "generator": concat!("SolidCraft ", env!("CARGO_PKG_VERSION"))},
        "scene": 0,
        "scenes": [{"name": name, "nodes": [0]}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials.iter().map(GlbMaterial::json).collect::<Vec<_>>(),
        "accessors": chunk.accessors,
        "bufferViews": chunk.views,
        "buffers": [{"byteLength": chunk.bin.len()}],
    });
    let used: std::collections::BTreeSet<String> = materials
        .iter()
        .filter_map(|m| m.json().get("extensions").and_then(Value::as_object).map(|e| e.keys().cloned().collect::<Vec<_>>()))
        .flatten()
        .collect();
    if !used.is_empty() {
        gltf["extensionsUsed"] = json!(used);
    }
    let json = serde_json::to_vec(&gltf).map_err(|e| IoError::Invalid(e.to_string()))?;
    container(&json, &chunk.bin)
}

/// A GLB container of `json` and `bin`, each chunk padded to four bytes (the JSON with spaces,
/// the binary with zeros), with every length checked to fit the format's 32 bits.
fn container(json: &[u8], bin: &[u8]) -> Result<Vec<u8>> {
    let pad = |n: usize| (4 - n % 4) % 4;
    let json_len = json.len() + pad(json.len());
    let bin_len = bin.len() + pad(bin.len());
    let total = 12usize
        .checked_add(8 + json_len)
        .and_then(|n| n.checked_add(if bin.is_empty() { 0 } else { 8 + bin_len }))
        .filter(|n| *n <= MAX_GLB_BYTES)
        .ok_or_else(|| IoError::Invalid(format!("the model is too large for a GLB file (at most {} MiB)", MAX_GLB_BYTES >> 20)))?;
    let u32_of = |n: usize| u32::try_from(n).map_err(|_| IoError::Invalid("the model is too large for a GLB file".into()));
    let mut out = Vec::new();
    out.try_reserve_exact(total).map_err(|_| IoError::Invalid("not enough memory for the GLB file".into()))?;
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&u32_of(total)?.to_le_bytes());
    out.extend_from_slice(&u32_of(json_len)?.to_le_bytes());
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(json);
    out.extend(std::iter::repeat_n(b' ', pad(json.len())));
    if !bin.is_empty() {
        out.extend_from_slice(&u32_of(bin_len)?.to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(bin);
        out.extend(std::iter::repeat_n(0u8, pad(bin.len())));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "glb_tests.rs"]
mod tests;
