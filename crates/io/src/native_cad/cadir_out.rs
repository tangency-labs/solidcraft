//! A design written as cadmpeg's neutral model (CADIR), for the writers cadmpeg has (OpenUSD and
//! Rhino): nothing passes through STEP text on the way. What each writer can hold differs, and
//! [`Writes`] says what the model is built with for it: the product structure or the bodies where
//! the design shows them, a mesh beside each exact shape or only for mesh bodies, looks as
//! appearances or as each body's colour, and colours linear or as SolidCraft keeps them (sRGB).
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
//! Each body: its exact B-rep ([`solidcraft_kernel::write_cadir`], from the body's record), and the
//! mesh the design is drawn with at the export's quality ([`crate::display_mesh_at`]) as a
//! tessellation of it, so the writer writes SolidCraft's mesh rather than meshing again. A mesh
//! body has the mesh alone, as is every body of an export asked for meshes only
//! ([`crate::CadmpegOptions::meshes_only`]). A B-rep CADIR cannot hold yet is written as its mesh,
//! with a warning naming the body.
//!
//! Looks: one appearance per distinct look, named after it, its colour linear (SolidCraft's are
//! sRGB, CADIR's as USD's linear) with the look's opacity as alpha, and the PBR parameters
//! (`metallic`, `roughness`, `ior`, the names `UsdPreviewSurface` gives its inputs) when the look
//! has them. A body's look is bound to the body; a face with a look of its own is bound to a
//! triangle group of the tessellation (the triangles meshed from it). It is not bound to the
//! B-rep face as well: cadmpeg 0.6.12 writes such a binding as a `GeomSubset` under the
//! `BrepArray` prim, which OpenUSD's validation refuses (no family name, and a parent that is not
//! imageable). A writer that takes no mesh beside the exact shape (STEP) has the face's look
//! bound to its B-rep face instead. A body with no look but a colour (an imported one) carries
//! that colour.
//!
//! What the view hides is written hidden: a placement hidden itself as a hidden occurrence, and
//! a body as a hidden body when it is hidden in every shown placement of its component. A body
//! hidden in some of those placements and shown in others is written shown, with a warning: the
//! file gives a component's body one visibility.
//!
//! Body names, for OpenUSD ([`Writes::named_bodies`]): cadmpeg 0.6.12 names the prim of a
//! component's only body after the placement when the component is placed once (the design's own
//! included), so a body named otherwise lost its name. Such a body is put in a definition of its
//! own, named after the component and placed once in the placement, with no offset and the
//! body's name: the file then holds a prim named after the placement and one named after the
//! body. A component placed more than once keeps cadmpeg's form, its only body's prim named after
//! the component: moving the body out would make each placement a component of its own when the
//! file is read back.
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

use super::CadmpegOptions;
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

/// What the model is built with for one of cadmpeg's writers.
#[derive(Clone, Copy)]
pub(crate) struct Writes {
    /// The format's name, for warnings.
    pub label: &'static str,
    /// Components as product definitions and placements as occurrences. Without them every body
    /// is written where the design shows it, with no product records at all.
    pub products: bool,
    /// A body with an exact shape carries its mesh too (else only mesh bodies have a mesh).
    pub brep_meshes: bool,
    /// Meshes are bound to their body; else each is a mesh of its own, with no body record (and
    /// no face groups, so a face's own look is not written).
    pub bound_meshes: bool,
    /// Looks as appearances bound to bodies and faces; else each body's colour alone.
    pub appearances: bool,
    /// Colours linear (OpenUSD's); else as SolidCraft keeps them, sRGB.
    pub linear: bool,
    /// Pcurves on planes kept; `false` leaves them out, for a writer that makes them from the
    /// edges itself and takes no other.
    pub planar_pcurves: bool,
    /// A component's only body named other than the prim the writer names after its one
    /// placement is put in a definition of its own, so its name reaches the file.
    pub named_bodies: bool,
    /// Each pcurve states its own parameter range (its coedge's), and each use whether it is
    /// declared isoparametric (never, as that is only a hint), for a writer that must have both.
    pub stated_pcurves: bool,
    /// The writer names each solid after its body; else a warning says the names are not written.
    pub solid_names: bool,
    /// Whether the writer takes a body's exact shape (the body alone in a model, planned by the
    /// writer); a body it refuses is written as its mesh, with a warning naming it and why.
    pub admit: Option<fn(&CadIr) -> Result<(), String>>,
}

impl Writes {
    /// OpenUSD: the product structure, each body's exact shape and mesh, looks as materials with
    /// linear colours.
    pub const USD: Writes = Writes {
        label: "OpenUSD",
        products: true,
        brep_meshes: true,
        bound_meshes: true,
        appearances: true,
        linear: true,
        planar_pcurves: true,
        named_bodies: true,
        stated_pcurves: false,
        solid_names: true,
        admit: None,
    };
}

/// A design (or the chosen bodies of it) as CADIR, with what could not be carried.
pub(crate) struct Built {
    pub ir: CadIr,
    pub warnings: Vec<String>,
}

/// [`design_for`] for OpenUSD ([`Writes::USD`]).
#[cfg(test)]
pub(crate) fn design(
    doc: &Document,
    model: &ModelState,
    world: &ModelState,
    bodies: &[String],
    options: &CadmpegOptions,
    name: &str,
    quality: MeshQuality,
) -> Result<Built, String> {
    design_for(doc, model, world, bodies, options, name, quality, Writes::USD)
}

/// The bodies of `world` (the design's model placed in the world) as CADIR: all of them as the
/// design's product structure when `bodies` is empty and the design has placements (`model`, the
/// model before placing, holds each component's bodies), else the chosen bodies (all when
/// `bodies` is empty) where the world has them, with what `options` asks for. `name` names
/// the root when the design's own name is blank. Each mesh at `quality`. Built for the writer
/// `writes` describes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn design_for(
    doc: &Document,
    model: &ModelState,
    world: &ModelState,
    bodies: &[String],
    options: &CadmpegOptions,
    name: &str,
    quality: MeshQuality,
    writes: Writes,
) -> Result<Built, String> {
    let _span = solidcraft_profile::span("io", "cadir_out.design");
    let root = clean(&doc.name).or_else(|| clean(name)).unwrap_or_else(|| "Design".into());
    let mut b = Builder::new(doc, quality, writes, &options.hidden);
    let assembly = bodies.is_empty() && !doc.occurrences.is_empty();
    if assembly && !writes.products {
        b.warnings.push(format!(
            "the components and placements are not written: cadmpeg's {} writer writes no assemblies, so each body is written where the design shows it",
            writes.label
        ));
    }
    let structured = assembly && writes.products;
    let unplaced = doc.occurrences.iter().find(|o| !solidcraft_doc::is_placement(&o.transform));
    b.physics = options.physics && writes.products && CadmpegOptions::physics_fits(doc, bodies);
    b.surfaces = &options.surfaces;
    // A picture is not attached to a body, so a part of the design cannot say which pictures are
    // its own: they are written with the whole design only.
    if bodies.is_empty() {
        b.decals = &options.decals;
    } else if !options.decals.is_empty() {
        b.warnings.push(format!(
            "the {} pictures shown on the model are not written: they are written only with the whole design, as a picture belongs to no body",
            options.decals.len()
        ));
    }
    b.optics = &options.optics;
    b.meshes_only = options.meshes_only;
    if options.physics && !b.physics {
        b.warnings
            .push("joints and mass are written only for the whole design with its components and rigid placements: no physics is written".into());
    }
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

/// SolidCraft's sRGB colour (bytes) and an opacity as a CADIR colour: linear when `linear` (as
/// OpenUSD's are), else the sRGB values themselves.
fn colour(rgb: [u8; 3], opacity: f64, linear: bool) -> Option<Color> {
    let channel = |v: u8| if linear { solidcraft_color::srgb::decode8(v) } else { f64::from(v) / 255.0 };
    let [r, g, b] = rgb.map(|v| channel(v).clamp(0.0, 1.0) as f32);
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
    writes: Writes,
    hidden: &'a super::Hidden,
    /// Joints, grounding and mass written ([`super::cadir_physics`]).
    physics: bool,
    /// Each component's bodies' masses in its frame, when [`Self::physics`].
    masses: HashMap<u64, Vec<crate::urdf::BodyMass>>,
    /// The image textures a render setup gives bodies ([`super::cadir_textures`]).
    surfaces: &'a [crate::SurfacePlan],
    /// The pictures shown on the model, each written as a body of its own.
    decals: &'a [crate::DecalPlan],
    /// The optics a render setup gives bodies.
    optics: &'a [crate::OpticsPlan],
    /// Each body as its mesh alone ([`crate::CadmpegOptions::meshes_only`]).
    meshes_only: bool,
    /// The appearances with optics made, by plain appearance (`None`: a body's colour) and
    /// optics.
    optical: HashMap<(Option<AppearanceId>, String), AppearanceId>,
    /// The decal bodies written so far.
    decal_count: usize,
    /// The texture images written.
    texture_assets: super::cadir_textures::Assets,
    /// The textured appearances made, by plain appearance (`None`: a body's colour, which the
    /// key then holds) and projection.
    textured: HashMap<(Option<AppearanceId>, String), AppearanceId>,
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
    fn new(doc: &'a Document, quality: MeshQuality, writes: Writes, hidden: &'a super::Hidden) -> Self {
        Builder {
            doc,
            quality,
            writes,
            hidden,
            physics: false,
            masses: HashMap::new(),
            surfaces: &[],
            decals: &[],
            decal_count: 0,
            optics: &[],
            meshes_only: false,
            optical: HashMap::new(),
            texture_assets: super::cadir_textures::Assets::default(),
            textured: HashMap::new(),
            ir: CadIr::empty(),
            warnings: Vec::new(),
            looks: HashMap::new(),
            bodies: 0,
            bindings: 0,
            tolerance: None,
            bytes: 0,
        }
    }

    fn finish(mut self) -> Result<Built, String> {
        if self.ir.model.bodies.is_empty() && self.ir.model.tessellations.is_empty() {
            return Err("there is no geometry to write".into());
        }
        if let Some(t) = self.tolerance.and_then(PositiveLength::new) {
            self.ir.tolerances.linear = t;
        }
        if !self.writes.planar_pcurves {
            without_planar_pcurves(&mut self.ir);
        }
        if self.writes.named_bodies {
            self.keep_body_names()?;
        }
        if self.writes.stated_pcurves {
            with_stated_pcurves(&mut self.ir);
        }
        if !self.writes.solid_names && self.ir.model.bodies.iter().any(|b| b.name.is_some()) {
            self.warnings.push(format!(
                "the bodies' names are not written: cadmpeg's {} writer writes each solid without a name (the components keep theirs)",
                self.writes.label
            ));
        }
        self.ir.finalize();
        Ok(Built { ir: self.ir, warnings: self.warnings })
    }

    /// Each definition placed once, holding one body and placing nothing, whose body is named
    /// other than the placement: the body moves to a definition of its own named as this one,
    /// placed in that placement with no offset and named after the body.
    fn keep_body_names(&mut self) -> Result<(), String> {
        let model = &self.ir.model;
        // Indexed once, so the pass stays linear in the definitions and placements.
        let names: HashMap<&BodyId, &str> = model.bodies.iter().filter_map(|b| b.name.as_deref().map(|n| (&b.id, n))).collect();
        let mut placing: HashMap<&ProductDefinitionId, Vec<(cadmpeg_ir::ids::OccurrenceId, Option<String>)>> = HashMap::new();
        for o in &model.occurrences {
            if let PrototypeReference::Local { definition } = &o.prototype {
                placing.entry(definition).or_default().push((o.id.clone(), o.name.clone()));
            }
        }
        let parents: std::collections::HashSet<&cadmpeg_ir::ids::OccurrenceId> = model
            .occurrences
            .iter()
            .filter_map(|o| match &o.parent {
                OccurrenceParent::Occurrence { occurrence } => Some(occurrence),
                _ => None,
            })
            .collect();
        let mut moves = Vec::new();
        for (index, d) in model.product_definitions.iter().enumerate() {
            let [body] = d.bodies.as_slice() else { continue };
            let Some(&name) = names.get(body) else { continue };
            let Some(placements) = placing.get(&d.id) else { continue };
            let [(placement, placed_as)] = placements.as_slice() else { continue };
            if !parents.contains(placement) && placed_as.as_deref() != Some(name) {
                moves.push((index, name.to_string(), placements.clone()));
            }
        }
        for (index, name, placements) in moves {
            let key = self.ir.model.product_definitions.len();
            let id: ProductDefinitionId = id("product", key)?;
            let Some(outer) = self.ir.model.product_definitions.get_mut(index) else { continue };
            let bodies = std::mem::take(&mut outer.bodies);
            let source_name = outer.source_name.clone();
            self.ir.model.product_definitions.push(ProductDefinition {
                id: id.clone(),
                kind: ProductDefinitionKind::Object,
                source_name,
                label: None,
                description: None,
                part_number: None,
                bom_properties: BTreeMap::new(),
                bodies,
                sketches: Vec::new(),
                native_ref: None,
            });
            for (parent, _) in placements {
                let key = self.ir.model.occurrences.len();
                self.occurrence(key, &id, OccurrenceParent::Occurrence { occurrence: parent }, 0, Transform::identity(), Some(name.clone()))?;
            }
        }
        Ok(())
    }

    /// The mass of body `b` (its geometry `geometry` in its component's frame) for component
    /// `component`, from its material's density and its closed mesh; a body that does not close
    /// adds none, with a warning.
    fn add_mass(&mut self, b: &ModelBody, geometry: &solidcraft_kernel::Body, component: u64) {
        match crate::mass_properties(geometry) {
            Ok(Some(mp)) => {
                // g/cm³ to kg/mm³.
                let rho = self.doc.density(&b.name) * 1e-6;
                self.masses.entry(component).or_default().push((rho * mp.volume, mp.centroid, mp.inertia.map(|r| r.map(|x| x * rho))));
            }
            Ok(None) => self.warnings.push(format!("{}: not a closed solid: it adds no mass", clean(&b.name).unwrap_or_else(|| "a body".into()))),
            Err(e) => self.warnings.push(format!("{}: its mass could not be measured: {e}", clean(&b.name).unwrap_or_else(|| "a body".into()))),
        }
    }

    /// The render setup's textures of model body `b` of component `component`, with the matrix
    /// from its definition frame to its mesh's, when every shown placement (`shown`) has them
    /// alike; else none, with a warning when some placement has them. `glass` leaves the metallic
    /// map out of the comparison, as it is left out of the file.
    fn surface(&mut self, b: &ModelBody, component: u64, shown: &[Vec<u64>], glass: bool) -> Option<(&'a crate::SurfacePlan, Mat)> {
        let surfaces = self.surfaces;
        let bound: Vec<&crate::SurfacePlan> = surfaces.iter().filter(|p| p.definition == b.definition_name() && p.component == component).collect();
        // The reference is a shown placement's: a hidden one's images do not count.
        let first = *bound.iter().find(|p| shown.contains(&p.path)).or(bound.first())?;
        let covered =
            !shown.is_empty() && shown.iter().all(|path| bound.iter().any(|p| &p.path == path && super::cadir_textures::same(p, first, glass)));
        if !covered {
            self.warnings.push(format!(
                "{}: render setup `{}` gives its placements different images (or some none): written with its look, as the file gives a component's body one mesh",
                clean(&b.name).unwrap_or_else(|| "a body".into()),
                first.setup
            ));
            return None;
        }
        let to_mesh = match self.doc.model_body_offset(b) {
            Some(offset) => solidcraft_doc::mat_mul(offset, &b.source_transform),
            None => b.source_transform,
        };
        Some((first, to_mesh))
    }

    /// Decal `d` with its corners at `corners` (in the frame of the definition that holds it): a
    /// body of its own named after it, its two-sided quad as its mesh and its image as its look.
    /// `None`, with a warning, when it cannot be written.
    fn decal(&mut self, d: &crate::DecalPlan, corners: [[f64; 3]; 4]) -> Result<Option<BodyId>, String> {
        let shown = clean(&d.name).unwrap_or_else(|| "a decal".into());
        let Some((points, normals, triangles, coordinates)) = super::cadir_textures::decal_quad(corners) else {
            self.warnings.push(format!("decal `{shown}`: its corners give no plane: left out"));
            return Ok(None);
        };
        let Some(textures) = super::cadir_textures::decal_references(&d.image, &mut self.texture_assets, &mut self.ir.model.assets) else {
            self.warnings.push(format!("decal `{shown}`: its image is neither PNG nor JPEG: left out"));
            return Ok(None);
        };
        // Its quad counts towards the file's size, as a body's mesh does.
        if self.charge(points.len(), triangles.len()).is_none() {
            return Err(format!(
                "decal `{shown}`: the meshes are too large for the file (at most {} MiB); export fewer bodies or pictures, or use standard quality",
                MAX_BYTES >> 20
            ));
        }
        let n = self.decal_count;
        self.decal_count += 1;
        let prefix = format!("solidcraft:decal-{n:09}");
        let body: BodyId = BodyId::try_from(format!("{prefix}:body#0")).map_err(|_| format!("{prefix}: not a CADIR identity"))?;
        let tessellation = cadmpeg_ir::tessellation::TessellationId::try_from(format!("{prefix}:tessellation#0"))
            .map_err(|_| format!("{prefix}: not a CADIR identity"))?;
        let positions = points.iter().map(|p| Point3::new(p[0], p[1], p[2])).collect();
        let normals = normals.iter().map(|v| Vector3::new(v[0], v[1], v[2])).collect();
        let lanes = TessellationMesh::from_list_lanes(positions, triangles, Some(normals)).map_err(|e| e.to_string())?;
        let mesh = Tessellation::new(tessellation.clone(), lanes, Vec::new())
            .map_err(|e| e.to_string())?
            .with_body(Some(body.clone()))
            .with_texture_coordinates(vec![coordinates])
            .map_err(|e| e.to_string())?;
        self.ir.model.bodies.push(Body {
            id: body.clone(),
            kind: BodyKind::General,
            regions: Vec::new(),
            transform: None,
            name: Some(shown.clone()),
            color: None,
            visible: None,
        });
        self.ir.model.tessellations.push(mesh);
        let appearance: AppearanceId = id("decal-appearance", n)?;
        self.ir.model.appearances.push(Appearance {
            id: appearance.clone(),
            name: Some(shown),
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: Color::new(1.0, 1.0, 1.0, 1.0),
            properties: BTreeMap::new(),
            textures,
        });
        self.bind(AppearanceTarget::Tessellation(tessellation.to_string()), appearance)?;
        Ok(Some(body))
    }

    /// The render setup's optics of model body `b` of component `component`, when every shown
    /// placement (`shown`) has them alike; else none, with a warning when some placement has them.
    fn optics_of(&mut self, b: &ModelBody, component: u64, shown: &[Vec<u64>]) -> Option<&'a crate::OpticsPlan> {
        let all = self.optics;
        let bound: Vec<&crate::OpticsPlan> = all.iter().filter(|p| p.definition == b.definition_name() && p.component == component).collect();
        // The reference is a shown placement's: a hidden one's optics do not count.
        let first = *bound.iter().find(|p| shown.contains(&p.path)).or(bound.first())?;
        let alike = |p: &crate::OpticsPlan| (p.ior, p.absorption_per_mm, p.thin_sheet) == (first.ior, first.absorption_per_mm, first.thin_sheet);
        let covered = !shown.is_empty() && shown.iter().all(|path| bound.iter().any(|p| &p.path == path && alike(p)));
        if !covered {
            self.warnings.push(format!(
                "{}: render setup `{}` gives its placements different optics (or some none): written without them, as the file gives a component's body one look",
                clean(&b.name).unwrap_or_else(|| "a body".into()),
                first.setup
            ));
            return None;
        }
        Some(first)
    }

    /// The appearance `plain` (else one of colour `colour`) with `optics`' properties, made once.
    fn with_optics(&mut self, plain: Option<AppearanceId>, colour: Option<Color>, optics: &crate::OpticsPlan) -> Result<AppearanceId, String> {
        let properties = super::cadir_textures::optics_properties(optics);
        // With no appearance of its own, the colour it falls back to tells two apart too.
        let fallback = if plain.is_none() { colour.map(|c| [c.r(), c.g(), c.b(), c.a()].map(f32::to_bits)) } else { None };
        let key = (plain.clone(), format!("{:?}", (properties.as_slice(), fallback)));
        if let Some(id) = self.optical.get(&key) {
            return Ok(id.clone());
        }
        let id: AppearanceId = id("optics-appearance", self.optical.len())?;
        let base = plain.as_ref().and_then(|p| self.ir.model.appearances.iter().find(|a| a.id == *p)).cloned();
        let mut appearance = base.unwrap_or_else(|| Appearance {
            id: id.clone(),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: colour,
            properties: BTreeMap::new(),
            textures: Vec::new(),
        });
        appearance.id = id.clone();
        appearance.name = Some(match &appearance.name {
            Some(n) => format!("{n} {}", optics.setup),
            None => optics.setup.clone(),
        });
        for (name, value) in properties {
            if let (Some(name), Some(value)) = (NonBlankString::new(name), FiniteReal::new(value)) {
                appearance.properties.insert(name, value);
            }
        }
        self.ir.model.appearances.push(appearance);
        self.optical.insert(key, id.clone());
        Ok(id)
    }

    /// The appearance `plain` (else one of colour `colour`) with `plan`'s images, made once.
    fn textured(
        &mut self,
        plain: Option<AppearanceId>,
        colour: Option<Color>,
        plan: &crate::SurfacePlan,
        refs: &[cadmpeg_ir::appearance::TextureRef],
    ) -> Result<AppearanceId, String> {
        // With no appearance of its own, the colour it falls back to tells two apart too.
        let fallback = if plain.is_none() { colour.map(|c| [c.r(), c.g(), c.b(), c.a()].map(f32::to_bits)) } else { None };
        let projection =
            format!("{:?}", (plan.origin, plan.u, plan.v, plan.flip_v, fallback, refs.iter().map(|r| (&r.slot, &r.asset_guid)).collect::<Vec<_>>()));
        let key = (plain.clone(), projection);
        if let Some(id) = self.textured.get(&key) {
            return Ok(id.clone());
        }
        let id: AppearanceId = id("textured-appearance", self.textured.len())?;
        let base = plain.as_ref().and_then(|p| self.ir.model.appearances.iter().find(|a| a.id == *p)).cloned();
        let mut appearance = base.unwrap_or_else(|| Appearance {
            id: id.clone(),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: colour,
            properties: BTreeMap::new(),
            textures: Vec::new(),
        });
        appearance.id = id.clone();
        appearance.name = Some(match &appearance.name {
            Some(n) => format!("{n} {}", plan.setup),
            None => plan.setup.clone(),
        });
        appearance.textures = refs.to_vec();
        self.ir.model.appearances.push(appearance);
        self.textured.insert(key, id.clone());
        Ok(id)
    }

    /// Body `id` written hidden.
    fn hide(&mut self, id: &BodyId) {
        if let Some(body) = self.ir.model.bodies.iter_mut().find(|x| x.id == *id) {
            body.visible = Some(false);
        }
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
            let meshes = self.ir.model.tessellations.len();
            let surface = self.surfaces.iter().find(|p| p.world == b.name).map(|p| (p, b.source_transform));
            let optics = self.optics.iter().find(|p| p.world == b.name);
            match self.body(b, &b.body, &b.name, surface, optics)? {
                Some(id) => {
                    if self.hidden.bodies.contains(&b.name) {
                        self.hide(&id);
                    }
                    ids.push(id);
                }
                // Written as a mesh of its own, which the writer gives no visibility.
                None if self.hidden.bodies.contains(&b.name) && self.ir.model.tessellations.len() > meshes => {
                    self.warnings.push(format!(
                        "{}: hidden, but written shown: cadmpeg's {} writer gives a mesh no visibility",
                        clean(&b.name).unwrap_or_else(|| "a body".into()),
                        self.writes.label
                    ));
                }
                None => {}
            }
        }
        for d in self.decals {
            if let Some(id) = self.decal(d, d.world)? {
                ids.push(id);
            }
        }
        if self.writes.products {
            let definition = self.definition(0, root, ids)?;
            self.occurrence(0, &definition, OccurrenceParent::Root {}, 0, Transform::identity(), Some(root.to_string()))?;
        }
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
        // The paths of each component's shown placements: a hidden placement hides what it
        // places, so a body's own state counts in the others only.
        let mut shown: HashMap<u64, Vec<Vec<u64>>> = HashMap::new();
        for (component, path, _) in doc.placements() {
            if !self.hidden.placements.iter().any(|h| !h.is_empty() && path.starts_with(h)) {
                shown.entry(component).or_default().push(path);
            }
        }
        let world_names = solidcraft_doc::WorldNames::new(doc, model);
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
            let placements = shown.get(&owner).map(Vec::as_slice).unwrap_or_default();
            let optics = self.optics_of(b, owner, placements);
            let surface = self.surface(b, owner, placements, optics.is_some());
            if let Some(id) = self.body(b, framed.as_ref().unwrap_or(&b.body), b.definition_name(), surface, optics)?
                && let Some(list) = owned.get_mut(key)
            {
                let paths = shown.get(&owner).map(Vec::as_slice).unwrap_or_default();
                let hidden_in = paths.iter().filter(|p| self.hidden.bodies.contains(&world_names.name(&b.name, owner, p))).count();
                if hidden_in > 0 && hidden_in == paths.len() {
                    self.hide(&id);
                } else if hidden_in > 0 {
                    self.warnings.push(format!(
                        "{}: hidden in {hidden_in} of the {} shown placements of its component: written shown, as the file gives a component's body one visibility",
                        clean(&b.name).unwrap_or_else(|| "a body".into()),
                        paths.len()
                    ));
                }
                list.push(id);
                if self.physics {
                    self.add_mass(b, framed.as_ref().unwrap_or(&b.body), owner);
                }
            }
        }
        for d in self.decals {
            let Some(&key) = keys.get(&d.component) else { continue };
            if let Some(id) = self.decal(d, d.local)?
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
        let mut placed_ids: HashMap<Vec<u64>, cadmpeg_ir::ids::OccurrenceId> = HashMap::new();
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
                if self.hidden.placements.contains(&placed)
                    && let Some(occurrence) = self.ir.model.occurrences.last_mut()
                {
                    occurrence.visible = Some(false);
                }
                placed_ids.insert(placed.clone(), id.clone());
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
        if self.physics {
            let components: HashMap<u64, ProductDefinitionId> =
                keys.iter().filter_map(|(component, key)| definitions.get(*key).map(|d| (*component, d.clone()))).collect();
            let physics = super::cadir_physics::build(doc, model, &placed_ids, &components, &self.masses)?;
            self.warnings.extend(physics.warnings);
            self.ir.model.assembly_joints.extend(physics.joints);
            self.ir.model.mass_properties.extend(physics.masses);
            self.ir.model.rigid_bodies.extend(physics.rigid_bodies);
            self.ir.model.articulations.extend(physics.articulations);
            self.ir.model.physics_scenes.extend(physics.scenes);
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
            base_color: colour(look.color, look.opacity, self.writes.linear),
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
    /// component's frame), named `name`: its B-rep and, where the writer takes one, its mesh.
    /// `None` when nothing of it could be written, with a warning naming it, or when it is
    /// written as a mesh of its own, with no body record ([`Writes::bound_meshes`]).
    fn body(
        &mut self,
        b: &ModelBody,
        geometry: &solidcraft_kernel::Body,
        name: &str,
        surface: Option<(&'a crate::SurfacePlan, Mat)>,
        optics: Option<&'a crate::OpticsPlan>,
    ) -> Result<Option<BodyId>, String> {
        let shown = clean(&b.name).unwrap_or_else(|| "a body".into());
        let n = self.bodies;
        self.bodies += 1;
        let prefix = format!("solidcraft:body-{n:09}");
        let name = clean(name).unwrap_or_else(|| shown.clone());
        let label = self.writes.label;
        // Each record face's CADIR face, when the exact shape is written.
        let mut brep_faces = Vec::new();
        let mut brep = if geometry.is_mesh() || self.meshes_only {
            None
        } else {
            match self.brep(geometry, &prefix, &name) {
                Ok(written) => {
                    brep_faces = written.faces;
                    Some(written.body)
                }
                Err(e) => {
                    self.warnings.push(format!("{shown}: written as its mesh only: its exact shape could not be: {e}"));
                    None
                }
            }
        };
        let mesh = if brep.is_some() && !self.writes.brep_meshes {
            None
        } else {
            let made = self.mesh(geometry, &shown);
            // Asked for meshes only, but the mesh cannot be made: the exact shape is what is left.
            let instead = shape_instead(&made, self.meshes_only, geometry.is_mesh()).then(|| self.brep(geometry, &prefix, &name).ok()).flatten();
            match (instead, made) {
                (Some(written), Err(Unmeshed::Failed(e))) => {
                    self.warnings
                        .push(format!("{shown}: its mesh could not be made ({e}): written as its exact shape, although the file is meshes only"));
                    brep_faces = written.faces;
                    brep = Some(written.body);
                    None
                }
                (_, made) => kept_without_mesh(made, brep.is_some(), &shown, &mut self.warnings)?,
            }
        };
        let has_brep = brep.is_some();
        // The body record just appended, so a look does not scan the list for its id.
        let (id, pushed) = match brep {
            Some(id) => (Some(id), None),
            None if self.writes.bound_meshes => {
                let id: BodyId = BodyId::try_from(format!("{prefix}:body#0")).map_err(|_| format!("{prefix}: not a CADIR identity"))?;
                let index = self.ir.model.bodies.len();
                self.ir.model.bodies.push(Body {
                    id: id.clone(),
                    kind: BodyKind::General,
                    regions: Vec::new(),
                    transform: None,
                    name: Some(name.clone()),
                    color: None,
                    visible: None,
                });
                (Some(id), Some(index))
            }
            None => (None, None),
        };
        // Looks: the body's, else its colour; and each face's own.
        let doc = self.doc;
        let look = doc.model_body_look(b);
        let mut face_looks: BTreeMap<u32, AppearanceId> = BTreeMap::new();
        let faces = doc.face_colors(b);
        // The body's own appearance, for its textured variant.
        let mut own: Option<AppearanceId> = None;
        match &id {
            Some(id) if self.writes.appearances => {
                match &look {
                    Some(look) => {
                        let mut a = self.appearance(look)?;
                        if let Some(o) = optics {
                            a = self.with_optics(Some(a), None, o)?;
                        }
                        own = Some(a.clone());
                        self.bind(AppearanceTarget::Body(id.clone()), a)?;
                    }
                    None => {
                        self.set_colour(id, b, pushed);
                        if let Some(o) = optics {
                            let colour = pushed
                                .and_then(|i| self.ir.model.bodies.get(i))
                                .filter(|x| x.id == *id)
                                .or_else(|| self.ir.model.bodies.iter().find(|x| x.id == *id))
                                .and_then(|x| x.color);
                            let a = self.with_optics(None, colour, o)?;
                            own = Some(a.clone());
                            self.bind(AppearanceTarget::Body(id.clone()), a)?;
                        }
                    }
                }
                if let Some(o) = optics {
                    self.warnings.push(format!(
                        "{shown}: the optics of render setup `{}` are written as the MaterialX shader's transmission: a reader of the preview surface alone shows it opaque",
                        o.setup
                    ));
                }
                if self.writes.bound_meshes {
                    for (face, look) in &faces {
                        let mut a = self.appearance(look)?;
                        if let Some(o) = optics {
                            a = self.with_optics(Some(a), None, o)?;
                        }
                        if let Ok(face) = u32::try_from(*face) {
                            face_looks.insert(face, a);
                        }
                    }
                }
            }
            Some(id) => {
                let linear = self.writes.linear;
                match &look {
                    Some(look) => {
                        let color = colour(look.color, look.opacity, linear);
                        match pushed.and_then(|i| self.ir.model.bodies.get_mut(i)) {
                            Some(body) => body.color = color,
                            None => {
                                if let Some(body) = self.ir.model.bodies.iter_mut().find(|x| x.id == *id) {
                                    body.color = color;
                                }
                            }
                        }
                    }
                    None => self.set_colour(id, b, pushed),
                }
                if !faces.is_empty() {
                    self.warnings
                        .push(format!("{shown}: the colours of its own faces are not written: cadmpeg's {label} writer writes one colour a body"));
                }
            }
            None => {
                if look.is_some() || doc.body_color(b).is_some() || !faces.is_empty() {
                    self.warnings
                        .push(format!("{shown}: written as a mesh without its name or colour: cadmpeg's {label} writer writes a mesh without them"));
                } else {
                    self.warnings.push(format!("{shown}: written as a mesh without its name: cadmpeg's {label} writer writes a mesh without one"));
                }
            }
        }
        let Some(mesh) = mesh else {
            // No mesh beside the exact shape (STEP): each face's own look is bound to its B-rep
            // face, as there are no triangles to bind it to.
            for (face, a) in face_looks {
                match brep_faces.get(face as usize).cloned().flatten() {
                    Some(f) => self.bind(AppearanceTarget::Face(f), a)?,
                    None => self.warnings.push(format!("{shown}: the colour of its face {face} is not written: the face is not in its exact shape")),
                }
            }
            return Ok(id);
        };
        // A render setup's textures: the mesh's coordinates in its images, and the body's and its
        // faces' appearances with the images, bound to the mesh (the exact shape keeps the plain
        // look).
        let mut textures = None;
        if let (Some((plan, to_mesh)), Some(id), true) = (surface, &id, self.writes.appearances) {
            match super::cadir_textures::coordinates(&mesh.positions, plan, &to_mesh) {
                Some(coordinates) => {
                    let mut refs =
                        super::cadir_textures::references(plan, &mut self.texture_assets, &mut self.ir.model.assets, &shown, &mut self.warnings);
                    // Glass transmits: a metallic map would drive OpenPBR's metalness, which takes
                    // the transmission away, so a body given optics is written without it.
                    if let Some(o) = optics
                        && refs.iter().any(|r| r.slot.starts_with("metallic"))
                    {
                        refs.retain(|r| !r.slot.starts_with("metallic"));
                        self.warnings.push(format!(
                            "{shown}: the metallic image of render setup `{}` is not written, as its optics make it glass, which a metal does not transmit",
                            o.setup
                        ));
                    }
                    if !refs.is_empty() {
                        let colour = self.ir.model.bodies.iter().find(|x| x.id == *id).and_then(|x| x.color);
                        let body_look = self.textured(own.clone(), colour, plan, &refs)?;
                        for a in face_looks.values_mut() {
                            *a = self.textured(Some(a.clone()), None, plan, &refs)?;
                        }
                        textures = Some((coordinates, body_look));
                    }
                }
                None => {
                    self.warnings.push(format!("{shown}: the projection of render setup `{}` is not finite: written without its images", plan.setup))
                }
            }
        }
        let (coordinates, textured_look) = textures.unzip();
        let written = self.tessellation(&prefix, id.as_ref(), &mesh, &face_looks, coordinates);
        if written.is_err() {
            // Not written, so its size is not the file's.
            let solidcraft_geom::Mesh { triangles, .. } = mesh;
            self.refund(mesh.positions.len(), triangles.len());
        }
        match written {
            Ok(()) => {
                if let Some(a) = textured_look {
                    self.bind(AppearanceTarget::Tessellation(format!("{prefix}:tessellation#0")), a)?;
                }
            }
            Err(e) if has_brep => self.warnings.push(format!("{shown}: its mesh was not written ({e}), so the file's writer meshes it")),
            Err(e) => {
                self.warnings.push(format!("{shown}: not written: {e}"));
                if let Some(id) = &id {
                    self.ir.model.bodies.retain(|x| x.id != *id);
                    self.ir.model.appearance_bindings.retain(|x| x.target != AppearanceTarget::Body(id.clone()));
                }
                return Ok(None);
            }
        }
        Ok(id)
    }

    /// A body with no look but a colour (an imported one) carries that colour. `pushed` is its
    /// record's index when this call appended it.
    fn set_colour(&mut self, id: &BodyId, b: &ModelBody, pushed: Option<usize>) {
        let doc = self.doc;
        if let Some(c) = doc.body_color(b) {
            let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            let color = colour(c.map(to8), doc.model_body_opacity(b), self.writes.linear);
            let body = match pushed {
                Some(i) => self.ir.model.bodies.get_mut(i).filter(|x| x.id == *id),
                None => self.ir.model.bodies.iter_mut().find(|x| x.id == *id),
            };
            if let Some(body) = body {
                body.color = color;
            }
        }
    }

    /// `geometry`'s exact shape written as body `name` under `prefix`, once the writer takes it
    /// ([`Writes::admit`]).
    fn brep(&mut self, geometry: &solidcraft_kernel::Body, prefix: &str, name: &str) -> Result<solidcraft_kernel::CadirWritten, String> {
        let record = solidcraft_kernel::brep_record(geometry).map_err(|e| e.to_string())?;
        let tolerance = record.tolerance;
        if let Some(admit) = self.writes.admit {
            let mut alone = CadIr::empty();
            solidcraft_kernel::write_cadir(&mut alone.model, &record, prefix, Some(name)).map_err(|e| e.to_string())?;
            if let Some(t) = PositiveLength::new(tolerance) {
                alone.tolerances.linear = t;
            }
            if !self.writes.planar_pcurves {
                without_planar_pcurves(&mut alone);
            }
            alone.finalize();
            admit(&alone).map_err(|e| format!("cadmpeg's {} writer cannot write it yet ({e})", self.writes.label))?;
        }
        let w = solidcraft_kernel::write_cadir(&mut self.ir.model, &record, prefix, Some(name)).map_err(|e| e.to_string())?;
        if tolerance.is_finite() && tolerance > 0.0 {
            self.tolerance = Some(self.tolerance.map_or(tolerance, |t| t.max(tolerance)));
        }
        Ok(w)
    }

    /// `geometry`'s mesh at the export's quality, within the file's size cap.
    fn mesh(&mut self, geometry: &solidcraft_kernel::Body, shown: &str) -> Result<solidcraft_geom::Mesh, Unmeshed> {
        let mesh = crate::display_mesh_at(geometry, self.quality).map_err(|e| Unmeshed::Failed(format!("could not be meshed: {e}")))?;
        let solidcraft_geom::Mesh { triangles, .. } = &mesh;
        if triangles.is_empty() {
            return Err(Unmeshed::Failed("could not be meshed: no face gave triangles".into()));
        }
        self.charge(mesh.positions.len(), triangles.len()).ok_or_else(|| {
            Unmeshed::TooLarge(format!(
                "{shown}: the meshes are too large for the file (at most {} MiB); export fewer bodies or use standard quality",
                MAX_BYTES >> 20
            ))
        })?;
        Ok(mesh)
    }

    /// `points` points and `triangles` triangles added to the meshes written; `None`, adding
    /// nothing, when they would take the meshes past [`MAX_BYTES`].
    fn charge(&mut self, points: usize, triangles: usize) -> Option<()> {
        let size = points
            .checked_mul(BYTES_PER_POINT)
            .zip(triangles.checked_mul(BYTES_PER_TRIANGLE))
            .and_then(|(p, t)| p.checked_add(t))
            .and_then(|n| n.checked_add(self.bytes))
            .filter(|n| *n <= MAX_BYTES)?;
        self.bytes = size;
        Some(())
    }

    /// Take back what [`Self::charge`] added for `points` points and `triangles` triangles, for
    /// a mesh it charged that is not written after all.
    fn refund(&mut self, points: usize, triangles: usize) {
        let size = points.saturating_mul(BYTES_PER_POINT).saturating_add(triangles.saturating_mul(BYTES_PER_TRIANGLE));
        self.bytes = self.bytes.saturating_sub(size);
    }

    /// `mesh` as the tessellation of body `body` (a mesh of its own when `None`), the triangles of
    /// each face in `face_looks` a triangle group bound to that face's appearance.
    fn tessellation(
        &mut self,
        prefix: &str,
        body: Option<&BodyId>,
        mesh: &solidcraft_geom::Mesh,
        face_looks: &BTreeMap<u32, AppearanceId>,
        coordinates: Option<cadmpeg_ir::tessellation::TextureCoordinates>,
    ) -> Result<(), String> {
        let solidcraft_geom::Mesh { triangles, tri_face, .. } = mesh;
        if triangles.is_empty() {
            return Err("its mesh has no triangles".into());
        }
        let id = cadmpeg_ir::tessellation::TessellationId::try_from(format!("{prefix}:tessellation#0"))
            .map_err(|_| format!("{prefix}: not a CADIR identity"))?;
        let positions: Vec<Point3> = mesh.positions.iter().map(|p| Point3::new(p.x, p.y, p.z)).collect();
        let normals: Option<Vec<Vector3>> = (mesh.normals.len() == mesh.positions.len()
            && mesh.normals.iter().all(|n| n.x.is_finite() && n.y.is_finite() && n.z.is_finite()))
        .then(|| mesh.normals.iter().map(|n| Vector3::new(n.x, n.y, n.z)).collect());
        let lanes = TessellationMesh::from_list_lanes(positions, triangles.clone(), normals).map_err(|e| e.to_string())?;
        let mut t = Tessellation::new(id.clone(), lanes, Vec::new()).map_err(|e| e.to_string())?.with_body(body.cloned());
        if let Some(coordinates) = coordinates {
            t = t.with_texture_coordinates(vec![coordinates]).map_err(|e| e.to_string())?;
        }
        if !face_looks.is_empty() && tri_face.len() == triangles.len() {
            let mut groups: BTreeMap<Option<u32>, Vec<u32>> = BTreeMap::new();
            for (i, f) in tri_face.iter().enumerate() {
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

/// `ir` without the pcurves of faces on planes: a writer that makes them from the edges takes no
/// other ([`Writes::planar_pcurves`]).
fn without_planar_pcurves(ir: &mut CadIr) {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use std::collections::HashSet;
    let planes: HashSet<&str> = ir
        .model
        .surfaces
        .iter()
        .filter(|s| matches!(s.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))))
        .map(|s| s.id.as_str())
        .collect();
    let planar_faces: HashSet<&str> = ir.model.faces.iter().filter(|f| planes.contains(f.surface.as_str())).map(|f| f.id.as_str()).collect();
    let planar_loops: HashSet<String> =
        ir.model.loops.iter().filter(|l| planar_faces.contains(l.face.as_str())).map(|l| l.id.as_str().to_string()).collect();
    let mut dropped: HashSet<String> = HashSet::new();
    for c in ir.model.coedges.iter_mut().filter(|c| planar_loops.contains(c.owner_loop.as_str())) {
        dropped.extend(c.pcurves.drain(..).map(|u| u.pcurve.as_str().to_string()));
    }
    ir.model.pcurves.retain(|p| !dropped.contains(p.id.as_str()));
}

/// Why a body's mesh was not made ([`Builder::mesh`]).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Unmeshed {
    /// Its display mesh could not be made, with why.
    Failed(String),
    /// The export's meshes would pass the file's size cap ([`MAX_BYTES`]), said in full.
    TooLarge(String),
}

/// Whether a body of a meshes-only export (`meshes_only`) whose mesh failed (`made`) is written
/// as its exact shape: only when it has one (it is not a mesh body, `is_mesh`), and only when the
/// mesh failed, not when it passed the size cap.
pub(crate) fn shape_instead(made: &Result<solidcraft_geom::Mesh, Unmeshed>, meshes_only: bool, is_mesh: bool) -> bool {
    meshes_only && !is_mesh && matches!(made, Err(Unmeshed::Failed(_)))
}

/// The mesh `made` for body `shown`, or none: a body with an exact B-rep whose display mesh could
/// not be made is written as its B-rep alone, with a warning, as one whose mesh the file cannot
/// take is (the file's writer meshes it). The size cap stays fatal, and so does a failed mesh of a
/// body with no exact shape, which has nothing else to be written as.
pub(crate) fn kept_without_mesh(
    made: Result<solidcraft_geom::Mesh, Unmeshed>,
    has_brep: bool,
    shown: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<solidcraft_geom::Mesh>, String> {
    match made {
        Ok(mesh) => Ok(Some(mesh)),
        Err(Unmeshed::Failed(e)) if has_brep => {
            warnings.push(format!("{shown}: its mesh was not written ({e}), so the file's writer meshes it"));
            Ok(None)
        }
        Err(Unmeshed::Failed(e)) => Err(format!("{shown}: {e}")),
        Err(Unmeshed::TooLarge(e)) => Err(e),
    }
}

/// Each pcurve of `ir` states the parameter range its one use runs over, and each use that says
/// nothing states it is not declared isoparametric (the flag is only a hint to a reader)
/// ([`Writes::stated_pcurves`]). A pcurve used by more than one coedge, or with no range on its
/// use, is left as it is.
fn with_stated_pcurves(ir: &mut CadIr) {
    use cadmpeg_ir::geometry::pcurve::PcurveMetadata;
    use cadmpeg_ir::units::FiniteVector;
    let mut ranges: HashMap<String, Option<[f64; 2]>> = HashMap::new();
    for u in ir.model.coedges.iter_mut().flat_map(|c| c.pcurves.iter_mut()) {
        if u.isoparametric.is_none() {
            u.isoparametric = Some(false);
        }
        let range = u.parameter_range.map(|r| {
            let [a, b] = r.endpoints();
            [a.min(b), a.max(b)]
        });
        // A second use makes the pcurve's own range ambiguous.
        ranges.entry(u.pcurve.as_str().to_string()).and_modify(|r| *r = None).or_insert(range);
    }
    for p in &mut ir.model.pcurves {
        if p.metadata != PcurveMetadata::default() {
            continue;
        }
        if let Some(range) = ranges.get(p.id.as_str()).copied().flatten().and_then(FiniteVector::new) {
            p.metadata = PcurveMetadata::general(None, Some(range), None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A decal's quad (eight points, four triangles) counts towards the file's size: near the cap
    /// it is refused, adding nothing; with room it is written and charged.
    #[test]
    fn a_decal_is_charged_to_the_size_cap() {
        let doc = Document::new("Decal");
        let hidden = crate::Hidden::default();
        let corners = [[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 5.0, 0.0], [0.0, 5.0, 0.0]];
        let image = crate::SurfaceMap { sha256: "f00d".into(), bytes: std::sync::Arc::from(&b"\x89PNG\r\n\x1a\n"[..]) };
        let plan = crate::DecalPlan { name: "Logo".into(), component: 0, local: corners, world: corners, image };
        let quad = 8 * BYTES_PER_POINT + 4 * BYTES_PER_TRIANGLE;
        let mut b = Builder::new(&doc, MeshQuality::Standard, Writes::USD, &hidden);
        b.bytes = MAX_BYTES - quad + 1;
        assert!(b.decal(&plan, corners).is_err());
        assert_eq!(b.bytes, MAX_BYTES - quad + 1);
        assert!(b.ir.model.tessellations.is_empty());
        b.bytes = MAX_BYTES - quad;
        assert!(b.decal(&plan, corners).unwrap().is_some());
        assert_eq!(b.bytes, MAX_BYTES);
    }

    /// A body whose mesh is charged but not written (its mesh has no triangles, which its
    /// tessellation refuses) takes its charge back: the cap counts only the meshes written.
    #[test]
    fn a_mesh_not_written_is_not_charged() {
        let doc = Document::new("Unwritten");
        let hidden = crate::Hidden::default();
        let mut b = Builder::new(&doc, MeshQuality::Standard, Writes::USD, &hidden);
        let mesh = solidcraft_geom::Mesh { positions: vec![solidcore_record::Vec3::new(0.0, 0.0, 0.0); 3], ..Default::default() };
        assert!(b.charge(mesh.positions.len(), mesh.triangles.len()).is_some());
        assert_eq!(b.bytes, 3 * BYTES_PER_POINT);
        assert!(b.tessellation("solidcraft:body-000000000", None, &mesh, &BTreeMap::new(), None).is_err());
        b.refund(mesh.positions.len(), mesh.triangles.len());
        assert_eq!(b.bytes, 0);
    }
}
