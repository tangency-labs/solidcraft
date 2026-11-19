//! A design written as cadmpeg's neutral model (CADIR), for the writers cadmpeg has (OpenUSD so
//! far): nothing passes through STEP text on the way.
//!
//! Product structure: each component is a product definition named after it, holding its bodies
//! in its own frame, and each placement of it an occurrence named after the occurrence, placed in
//! its parent's frame (millimetres). CADIR's occurrences are the placed instances, so a component
//! inside a sub-assembly placed twice has an occurrence under each; the definition is still one,
//! and a writer writes a definition placed more than once once. The design itself is the root
//! definition, placed once at the root and named after the design. With bodies chosen, or a
//! placement that is not a rigid one (a mirror, or a number that is not finite), the bodies are
//! written where the design shows them, all in that root definition.
//!
//! Each body: its exact B-rep ([`solidcore_cadir::write_body`], from the body's record), and the
//! mesh the design is drawn with at the export's quality ([`crate::display_mesh_at`]) as a
//! tessellation of it, so the writer writes SolidCraft's mesh rather than meshing again. A mesh
//! body has the mesh alone. A B-rep CADIR cannot hold yet is written as its mesh, with a warning
//! naming the body.
//!
//! Looks: one appearance per distinct look, named after it, its colour linear (SolidCraft's are
//! sRGB, CADIR's as USD's linear) with the look's opacity as alpha, and the PBR parameters
//! (`metallic`, `roughness`, `ior`, the names `UsdPreviewSurface` gives its inputs) when the look
//! has them. A body's look is bound to the body; a face with a look of its own is bound to a
//! triangle group of the tessellation (the triangles meshed from it). It is not bound to the
//! B-rep face as well: cadmpeg 0.6.12 writes such a binding as a `GeomSubset` under the
//! `BrepArray` prim, which OpenUSD's validation refuses (no family name, and a parent that is not
//! imageable). A body with no look but a colour (an imported one) carries that colour.
//!
//! The design is untrusted: every placement is checked, the placements walked are capped as the
//! design's own walk caps them, and the meshes' size is capped ([`MAX_BYTES`]).

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{AppearanceId, BodyId, ProductDefinitionId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::products::{Occurrence, OccurrenceParent, ProductDefinition, ProductDefinitionKind, PrototypeReference};
use cadmpeg_ir::scalar::{FiniteReal, PositiveLength};
use cadmpeg_ir::tessellation::{Tessellation, TessellationMesh, TessellationTriangleGroup};
use cadmpeg_ir::topology::{Body, BodyKind, Color};
use cadmpeg_ir::transform::Transform;
use solidcraft_doc::appearance::Look;
use solidcraft_doc::{Document, Mat, ModelBody, ModelState};

use crate::MeshQuality;

/// Largest total size of the meshes written, estimated as the text layer holds them (about 40
/// bytes a point and 30 a triangle): a model meshed far too finely is refused at the first mesh
/// that passes it, before that mesh goes into the model, as a GLB is
/// ([`crate::glb::MAX_GLB_BYTES`]). Each mesh's own size is bounded by its quality's chord
/// tolerance, which is a fraction of the body's size.
pub(crate) const MAX_BYTES: usize = 1 << 30;
const BYTES_PER_POINT: usize = 40;
const BYTES_PER_TRIANGLE: usize = 30;

/// Longest name written (prims are named after them).
const MAX_NAME: usize = 128;

/// A design (or the chosen bodies of it) as CADIR, with what could not be carried.
pub(crate) struct Built {
    pub ir: CadIr,
    pub warnings: Vec<String>,
}

/// The bodies of `world` (the design's model placed in the world) as CADIR: all of them as the
/// design's product structure when `bodies` is empty and the design has placements (`model`, the
/// model before placing, holds each component's bodies), else the chosen bodies (all when
/// `bodies` is empty) where the world has them. `name` names the root when the design's own name
/// is blank. Each mesh at `quality`.
pub(crate) fn design(
    doc: &Document,
    model: &ModelState,
    world: &ModelState,
    bodies: &[String],
    name: &str,
    quality: MeshQuality,
) -> Result<Built, String> {
    let _span = solidcraft_profile::span("io", "cadir_out.design");
    let root = clean(&doc.name).or_else(|| clean(name)).unwrap_or_else(|| "Design".into());
    let mut b = Builder::new(doc, quality);
    let structured = bodies.is_empty() && !doc.occurrences.is_empty();
    let unplaced = doc.occurrences.iter().find(|o| !solidcraft_doc::is_placement(&o.transform));
    match (structured, unplaced) {
        (true, None) => b.assembly(model, &root)?,
        (true, Some(o)) => {
            b.warnings.push(format!(
                "{}: its placement is not a rigid one, so every body is written where the design shows it, without the components",
                o.name
            ));
            b.flat(world, bodies, &root)?;
        }
        (false, _) => b.flat(world, bodies, &root)?,
    }
    b.finish()
}

/// A name as written: trimmed, without control characters, at most [`MAX_NAME`] characters;
/// `None` when nothing is left.
fn clean(s: &str) -> Option<String> {
    let s: String = s.trim().chars().filter(|c| !c.is_control()).take(MAX_NAME).collect();
    (!s.trim().is_empty()).then(|| s.trim().to_string())
}

/// An id of this design's model: `solidcraft:design:<kind>#<key>`, the key padded so the model's
/// arenas, sorted by id, keep the order they were made in.
fn id<T: TryFrom<String>>(kind: &str, key: usize) -> Result<T, String> {
    let s = format!("solidcraft:design:{kind}#{key:09}");
    T::try_from(s.clone()).map_err(|_| format!("{s} is not a CADIR identity"))
}

/// `m` (column-major, mm) as a CADIR transform, when finite.
fn transform(m: &Mat) -> Option<Transform> {
    let row = |r: usize| [m[0][r], m[1][r], m[2][r], m[3][r]];
    Transform::affine([row(0), row(1), row(2)])
}

/// SolidCraft's sRGB colour (bytes) and an opacity as CADIR's linear colour.
fn linear(rgb: [u8; 3], opacity: f64) -> Option<Color> {
    let [r, g, b] = rgb.map(|v| solidcraft_color::srgb::decode8(v).clamp(0.0, 1.0) as f32);
    let a = if opacity.is_finite() { opacity.clamp(0.0, 1.0) as f32 } else { 1.0 };
    Color::new(r, g, b, a)
}

/// What makes two looks one appearance: every value written.
type LookKey = (String, [u8; 3], u64, Option<[u64; 3]>);

fn look_key(look: &Look) -> LookKey {
    let pbr = look.pbr.map(|p| [p.metallic().to_bits(), p.roughness().to_bits(), p.dielectric_ior().to_bits()]);
    (look.name.clone(), look.color, look.opacity.to_bits(), pbr)
}

struct Builder<'a> {
    doc: &'a Document,
    quality: MeshQuality,
    ir: CadIr,
    warnings: Vec<String>,
    looks: HashMap<LookKey, AppearanceId>,
    bodies: usize,
    bindings: usize,
    /// The loosest tolerance of the records written.
    tolerance: Option<f64>,
    /// The estimated size of the meshes so far.
    bytes: usize,
}

impl<'a> Builder<'a> {
    fn new(doc: &'a Document, quality: MeshQuality) -> Self {
        Builder { doc, quality, ir: CadIr::empty(), warnings: Vec::new(), looks: HashMap::new(), bodies: 0, bindings: 0, tolerance: None, bytes: 0 }
    }

    fn finish(mut self) -> Result<Built, String> {
        if self.ir.model.bodies.is_empty() {
            return Err("there is no geometry to write".into());
        }
        if let Some(t) = self.tolerance.and_then(PositiveLength::new) {
            self.ir.tolerances.linear = t;
        }
        self.ir.finalize();
        Ok(Built { ir: self.ir, warnings: self.warnings })
    }

    /// A product definition named `name` holding `bodies`, with its id.
    fn definition(&mut self, key: usize, name: &str, bodies: Vec<BodyId>) -> Result<ProductDefinitionId, String> {
        let id: ProductDefinitionId = id("product", key)?;
        self.ir.model.product_definitions.push(ProductDefinition {
            id: id.clone(),
            kind: ProductDefinitionKind::Part,
            source_name: Some(name.to_string()),
            label: None,
            description: None,
            part_number: None,
            bom_properties: BTreeMap::new(),
            bodies,
            sketches: Vec::new(),
            native_ref: None,
        });
        Ok(id)
    }

    /// Occurrence number `key` of `definition` in `parent`, placed there by `placement`.
    fn occurrence(
        &mut self,
        key: usize,
        definition: &ProductDefinitionId,
        parent: OccurrenceParent,
        ordinal: u32,
        placement: Transform,
        name: Option<String>,
    ) -> Result<cadmpeg_ir::ids::OccurrenceId, String> {
        let id: cadmpeg_ir::ids::OccurrenceId = id("occurrence", key)?;
        self.ir.model.occurrences.push(Occurrence {
            id: id.clone(),
            prototype: PrototypeReference::Local { definition: definition.clone() },
            parent,
            ordinal,
            transform: placement,
            linked_prototype: None,
            scale: [FiniteReal::ONE; 3],
            name,
            visible: None,
            link: None,
            native_ref: None,
        });
        Ok(id)
    }

    /// The chosen bodies of `world` (all when `bodies` is empty), where it has them, in one root
    /// definition named `root`.
    fn flat(&mut self, world: &ModelState, bodies: &[String], root: &str) -> Result<(), String> {
        let sel = crate::selected(world, bodies).map_err(|e| e.to_string())?;
        let mut ids = Vec::with_capacity(sel.len());
        for b in sel {
            if let Some(id) = self.body(b, &b.body, &b.name)? {
                ids.push(id);
            }
        }
        let definition = self.definition(0, root, ids)?;
        self.occurrence(0, &definition, OccurrenceParent::Root {}, 0, Transform::identity(), Some(root.to_string()))?;
        Ok(())
    }

    /// The design's components as definitions and its placements as occurrences, the design
    /// itself the root definition named `root`. `model` is the model before placing: each body
    /// in its component's frame (taken there by its offset, as the world placement takes it).
    fn assembly(&mut self, model: &ModelState, root: &str) -> Result<(), String> {
        let doc = self.doc;
        // Definition keys: 0 the design, then each component in order.
        let mut keys: HashMap<u64, usize> = HashMap::new();
        keys.insert(0, 0);
        let mut names = vec![root.to_string()];
        for c in &doc.components {
            if keys.contains_key(&c.id) {
                continue;
            }
            keys.insert(c.id, names.len());
            names.push(clean(&c.name).unwrap_or_else(|| format!("Component {}", c.id)));
        }
        let mut owned: Vec<Vec<BodyId>> = vec![Vec::new(); names.len()];
        let owners = doc.model_body_components(model);
        for (b, owner) in model.bodies.iter().zip(owners) {
            let Some(&key) = keys.get(&owner) else { continue };
            let framed = match doc.model_body_offset(b) {
                Some(offset) if !solidcraft_doc::is_identity(offset) => match solidcraft_kernel::transform_matrix(&b.body, *offset) {
                    Ok(f) => Some(f),
                    Err(e) => {
                        self.warnings.push(format!("{}: not written: it could not be taken into its component's frame: {e}", b.name));
                        continue;
                    }
                },
                _ => None,
            };
            if let Some(id) = self.body(b, framed.as_ref().unwrap_or(&b.body), b.definition_name())?
                && let Some(list) = owned.get_mut(key)
            {
                list.push(id);
            }
        }
        let mut definitions = Vec::with_capacity(names.len());
        for (key, (name, bodies)) in names.iter().zip(owned).enumerate() {
            definitions.push(self.definition(key, name, bodies)?);
        }
        let Some(design) = definitions.first().cloned() else { return Err("the design has no root".into()) };
        // The placed instances, walked from the root, as the design's own walk caps them. Each
        // parent's children keep their ordinal among all its children; one whose component has
        // no definition is left out here, once, not on every instance of its parent.
        let mut children: HashMap<u64, Vec<(u32, &solidcraft_doc::Occurrence, ProductDefinitionId)>> = HashMap::new();
        let mut counts: HashMap<u64, u32> = HashMap::new();
        for o in &doc.occurrences {
            let count = counts.entry(o.parent).or_default();
            let ordinal = *count;
            *count = count.saturating_add(1);
            if let Some(definition) = keys.get(&o.component).and_then(|k| definitions.get(*k)) {
                children.entry(o.parent).or_default().push((ordinal, o, definition.clone()));
            }
        }
        // The placements written are exactly those the design's own walk reaches
        // (`Document::placements`, which the display and the world model take), capped where it
        // caps them: a placement the design shows is written, and none it leaves out.
        let walked: HashSet<Vec<u64>> = doc.placements().into_iter().map(|(_, path, _)| path).collect();
        let mut cut = !doc.placements_complete();
        let first = self.occurrence(0, &design, OccurrenceParent::Root {}, 0, Transform::identity(), Some(root.to_string()))?;
        let mut stack = vec![(0u64, first, Vec::new())];
        let mut made = 1usize;
        while let Some((component, parent, path)) = stack.pop() {
            for (ordinal, o, definition) in children.get(&component).into_iter().flatten() {
                let mut placed: Vec<u64> = path.clone();
                placed.push(o.id);
                if !walked.contains(&placed) {
                    cut = true;
                    continue;
                }
                let Some(placement) = transform(&o.transform) else {
                    return Err(format!("{}: its placement is not a finite number", o.name));
                };
                let parent = OccurrenceParent::Occurrence { occurrence: parent.clone() };
                let id = self.occurrence(made, definition, parent, *ordinal, placement, clean(&o.name))?;
                made += 1;
                stack.push((o.component, id, placed));
            }
        }
        if cut {
            self.warnings.push(format!(
                "the assembly is too large or too deep: placements past {} levels or {} in all were left out",
                solidcraft_doc::MAX_PLACEMENT_DEPTH,
                solidcraft_doc::MAX_PLACEMENTS
            ));
        }
        Ok(())
    }

    /// The appearance of `look`, made the first time it is asked for.
    fn appearance(&mut self, look: &Look) -> Result<AppearanceId, String> {
        let key = look_key(look);
        if let Some(id) = self.looks.get(&key) {
            return Ok(id.clone());
        }
        let id: AppearanceId = id("appearance", self.looks.len())?;
        let mut properties = BTreeMap::new();
        if let Some(p) = look.pbr {
            for (k, v) in [("metallic", p.metallic()), ("roughness", p.roughness()), ("ior", p.dielectric_ior())] {
                if let (Some(k), Some(v)) = (NonBlankString::new(k), FiniteReal::new(v)) {
                    properties.insert(k, v);
                }
            }
        }
        self.ir.model.appearances.push(Appearance {
            id: id.clone(),
            name: clean(&look.name),
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: linear(look.color, look.opacity),
            properties,
            textures: Vec::new(),
        });
        self.looks.insert(key, id.clone());
        Ok(id)
    }

    fn bind(&mut self, target: AppearanceTarget, appearance: AppearanceId) -> Result<(), String> {
        let id = id("binding", self.bindings)?;
        self.bindings += 1;
        self.ir.model.appearance_bindings.push(AppearanceBinding {
            id,
            target,
            appearance,
            source_entity_id: None,
            object_type: None,
            visible: None,
            channels: BTreeMap::new(),
        });
        Ok(())
    }

    /// Body `b` (its looks), its geometry `geometry` (the body itself, or taken into its
    /// component's frame), named `name`: its B-rep and its mesh. `None` when nothing of it could
    /// be written, with a warning naming it.
    fn body(&mut self, b: &ModelBody, geometry: &solidcraft_kernel::Body, name: &str) -> Result<Option<BodyId>, String> {
        let shown = clean(&b.name).unwrap_or_else(|| "a body".into());
        let mesh = crate::display_mesh_at(geometry, self.quality).map_err(|e| format!("{shown}: could not be meshed: {e}"))?;
        let size = mesh
            .positions
            .len()
            .checked_mul(BYTES_PER_POINT)
            .zip(mesh.triangles.len().checked_mul(BYTES_PER_TRIANGLE))
            .and_then(|(p, t)| p.checked_add(t))
            .and_then(|n| n.checked_add(self.bytes))
            .filter(|n| *n <= MAX_BYTES)
            .ok_or_else(|| {
                format!(
                    "{shown}: the meshes are too large for the file (at most {} MiB); export fewer bodies or use standard quality",
                    MAX_BYTES >> 20
                )
            })?;
        self.bytes = size;
        let n = self.bodies;
        self.bodies += 1;
        let prefix = format!("solidcraft:body-{n:09}");
        let name = clean(name).unwrap_or_else(|| shown.clone());
        let mut has_brep = false;
        let body = if geometry.is_mesh() {
            None
        } else {
            match solidcraft_kernel::brep_record(geometry).map_err(|e| e.to_string()).and_then(|record| {
                let w = solidcore_cadir::write_body(&mut self.ir.model, &record, &prefix, Some(&name)).map_err(|e| e.to_string())?;
                Ok((w, record.tolerance))
            }) {
                Ok((w, tolerance)) => {
                    if tolerance.is_finite() && tolerance > 0.0 {
                        self.tolerance = Some(self.tolerance.map_or(tolerance, |t| t.max(tolerance)));
                    }
                    has_brep = true;
                    Some(w.body)
                }
                Err(e) => {
                    self.warnings.push(format!("{shown}: written as its mesh only: its exact shape could not be: {e}"));
                    None
                }
            }
        };
        let id = match body {
            Some(id) => id,
            None => {
                let id: BodyId = BodyId::try_from(format!("{prefix}:body#0")).map_err(|_| format!("{prefix}: not a CADIR identity"))?;
                self.ir.model.bodies.push(Body {
                    id: id.clone(),
                    kind: BodyKind::General,
                    regions: Vec::new(),
                    transform: None,
                    name: Some(name.clone()),
                    color: None,
                    visible: None,
                });
                id
            }
        };
        // Looks: the body's, else its colour; and each face's own.
        let doc = self.doc;
        match doc.model_body_look(b) {
            Some(look) => {
                let a = self.appearance(&look)?;
                self.bind(AppearanceTarget::Body(id.clone()), a)?;
            }
            None => {
                if let Some(c) = doc.body_color(b) {
                    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    let color = linear(c.map(to8), doc.model_body_opacity(b));
                    if let Some(body) = self.ir.model.bodies.iter_mut().find(|x| x.id == id) {
                        body.color = color;
                    }
                }
            }
        }
        let mut face_looks: BTreeMap<u32, AppearanceId> = BTreeMap::new();
        for (face, look) in doc.face_colors(b) {
            let a = self.appearance(&look)?;
            if let Ok(face) = u32::try_from(face) {
                face_looks.insert(face, a);
            }
        }
        match self.tessellation(&prefix, &id, &mesh, &face_looks) {
            Ok(()) => {}
            Err(e) if has_brep => self.warnings.push(format!("{shown}: its mesh was not written ({e}), so the file's writer meshes it")),
            Err(e) => {
                self.warnings.push(format!("{shown}: not written: {e}"));
                self.ir.model.bodies.retain(|x| x.id != id);
                self.ir.model.appearance_bindings.retain(|x| x.target != AppearanceTarget::Body(id.clone()));
                return Ok(None);
            }
        }
        Ok(Some(id))
    }

    /// `mesh` as the tessellation of body `body`, the triangles of each face in `face_looks`
    /// a triangle group bound to that face's appearance.
    fn tessellation(
        &mut self,
        prefix: &str,
        body: &BodyId,
        mesh: &solidcraft_geom::Mesh,
        face_looks: &BTreeMap<u32, AppearanceId>,
    ) -> Result<(), String> {
        if mesh.triangles.is_empty() {
            return Err("its mesh has no triangles".into());
        }
        let id = cadmpeg_ir::tessellation::TessellationId::try_from(format!("{prefix}:tessellation#0"))
            .map_err(|_| format!("{prefix}: not a CADIR identity"))?;
        let positions: Vec<Point3> = mesh.positions.iter().map(|p| Point3::new(p.x, p.y, p.z)).collect();
        let normals: Option<Vec<Vector3>> = (mesh.normals.len() == mesh.positions.len()
            && mesh.normals.iter().all(|n| n.x.is_finite() && n.y.is_finite() && n.z.is_finite()))
        .then(|| mesh.normals.iter().map(|n| Vector3::new(n.x, n.y, n.z)).collect());
        let lanes = TessellationMesh::from_list_lanes(positions, mesh.triangles.clone(), normals).map_err(|e| e.to_string())?;
        let mut t = Tessellation::new(id.clone(), lanes, Vec::new()).map_err(|e| e.to_string())?.with_body(Some(body.clone()));
        if !face_looks.is_empty() && mesh.tri_face.len() == mesh.triangles.len() {
            let mut groups: BTreeMap<Option<u32>, Vec<u32>> = BTreeMap::new();
            for (i, f) in mesh.tri_face.iter().enumerate() {
                let Ok(i) = u32::try_from(i) else { return Err("its mesh has too many triangles".into()) };
                let group = face_looks.contains_key(f).then_some(*f);
                groups.entry(group).or_default().push(i);
            }
            let partition: Vec<TessellationTriangleGroup> = groups
                .iter()
                .map(|(f, triangles)| TessellationTriangleGroup { source_id: f.map(|f| format!("face{f}")), triangles: triangles.clone() })
                .collect();
            t = t.with_triangle_groups(partition).map_err(|e| e.to_string())?;
            for f in groups.keys().flatten() {
                if let Some(a) = face_looks.get(f).cloned() {
                    self.bind(AppearanceTarget::TriangleGroup { tessellation: id.to_string(), group: format!("face{f}") }, a)?;
                }
            }
        }
        self.ir.model.tessellations.push(t);
        Ok(())
    }
}
