//! Timeline evaluation.

#[path = "eval_overlap.rs"]
mod overlap;
#[path = "placed_mesh.rs"]
mod placed_mesh;
#[path = "placement_reuse.rs"]
mod placement_reuse;

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use solidcore_record::brep::part::Contact;
use solidcore_record::{Plane, Region2, Vec2, Vec3};
use solidcraft_geom::tess::TessPlan;
use solidcraft_geom::{Mesh, UnitVec3, regions_apart};
use solidcraft_kernel::{self as kernel, Body, BoolOp};
use solidcraft_sketch::{CurveKind, Profile, SolveReport, find_profiles};

use crate::expr::{self, Kind, Value};
use crate::lineage::Traced;
use crate::{AxisRef, DocError, Document, Feature, FeatureId, FeatureKind, HoleKind, Operation, PlaneRef, ProfileSel, Result};

type JointFaces = (Body, kernel::Result<Arc<[kernel::FaceInfo]>>);

/// A body's face measurements worked out so far ([`ModelBody::faces_at`]), the latest first:
/// the chord tolerance (its bits), the body measured and what was found.
type MeasuredFaces = Vec<(u64, Body, kernel::Result<Arc<[kernel::FaceInfo]>>)>;

/// How many face measurements a body keeps ([`ModelBody::faces_at`]): callers ask at a
/// tolerance worked out from the body, so one body is asked at very few.
const FACES_KEPT: usize = 4;

/// A body's isoparametric curves worked out so far ([`ModelBody::iso_lines`]), with the body they
/// were worked out on: by face, count each way and display quality.
type IsoLines = (Option<Body>, std::collections::HashMap<(usize, usize, usize), Arc<[kernel::IsoLine]>>);

/// A body in the evaluated model.
#[derive(Clone, Debug)]
pub struct ModelBody {
    pub name: String,
    /// Definition identity for a computed placement; never parsed from its display name.
    pub source_name: Option<String>,
    /// Definition geometry coordinates to this computed body's frame.
    pub source_transform: crate::Mat,
    pub body: Body,
    /// Feature that created it.
    pub feature: u64,
    /// Sketch line that produced this frame member, independent of its current body name.
    pub(crate) frame_member_line: Option<Arc<str>>,
    // Timeline snapshots are cloned before meshes are requested. Share the cell itself so
    // meshing any snapshot also warms every unchanged copy of this body.
    /// The box of the body's display mesh ([`ModelBody::mesh_bounds`]), worked out once: framing
    /// the view goes over every body, and a placed copy's box moves every point of its part's
    /// mesh. Shared by the copies of this body that keep its placement.
    bounds: Arc<OnceLock<solidcraft_geom::Aabb3>>,
    mesh: Arc<OnceLock<Arc<Mesh>>>,
    /// Joint face measurements use their own tolerance, independent of display quality.
    /// Shared by snapshots of this geometry; the body identity guards direct replacement.
    joint_faces: Arc<OnceLock<JointFaces>>,
    /// [`ModelBody::faces_at`] worked out so far. Shared by snapshots of this geometry; each
    /// measurement's body guards direct replacement.
    faces: Arc<std::sync::Mutex<MeasuredFaces>>,
    /// Lazy canonical reference triangles for a proper rigid occurrence placement.
    mesh_placement: Option<Arc<placed_mesh::PlacedMesh>>,
    /// The mesh a cam's profile is read from: the body's edges at the cam's own tolerance. Made
    /// once and kept, so a solve that works the cam's table out again does not mesh the body
    /// again (#3389).
    cam_mesh: Arc<OnceLock<Arc<Mesh>>>,
    /// For a mirrored copy: its faces as a world state shows them, a rigid move of the source's
    /// [`placed_mesh::OppositeHand`]. Display only: the model reads the copy's own triangles.
    mirrored_mesh: Option<Arc<placed_mesh::PlacedMesh>>,
    /// The first mirrored copy made of this body, shared by its clones.
    opposite_hand: Arc<OnceLock<placed_mesh::OppositeHand>>,
    /// The display plan (none when the body has no plan: a mesh body, or one the planner
    /// refuses), shared like the mesh. A GPU viewport draws it.
    plan: Arc<[OnceLock<Option<Arc<TessPlan>>>; 4]>,
    /// Each display plan in the form the GPU evaluates ([`crate::gpu_plan`]), made once from it
    /// and shared like it, so the viewport sees the same allocation every frame.
    gpu_plan: Arc<[OnceLock<Option<Arc<solidcore_tess::TessPlan>>>; 4]>,
    /// The plan evaluated on the CPU (none without a plan), shared like the mesh: the triangles a
    /// GPU viewport draws, so a highlight over a face lies exactly on it.
    plan_mesh: Arc<[OnceLock<Option<Arc<Mesh>>>; 4]>,
    /// Identity of this body's content for the evaluation cache: the key of the evaluation
    /// that produced it (kept while later features pass it through unchanged).
    key: u64,
    /// Persistent face names (worked out on first use).
    pub(crate) names: crate::naming::NameCell,
    /// How the feature being evaluated made or changed it (read when its names are set up).
    pub(crate) made: crate::lineage::Made,
    /// The sketch frame each face takes, where what built the body gives one (a sheet's panel's
    /// face: the flat pattern's, see `SurfaceSheet::frames`), by face (see
    /// [`ModelBody::sketch_frame`]).
    pub(crate) sketch_frames: Option<Arc<Vec<Option<Plane>>>>,
    /// The component the body belongs to (set after each feature).
    pub component: u64,
    /// Its owner is `component`, not looked up by name or feature: a timeline transfer has
    /// converted it into that owner's frame, or it was split off a body there.
    pub transferred: bool,
    /// For a piece a cut or intersect split off a body (past its first, which keeps the body's
    /// name): that body's name, so the piece can take its material and look.
    pub split_from: Option<String>,
    /// The look it was imported with, carried through later features.
    pub imported: Option<Arc<crate::appearance::ImportedLook>>,
    /// A component's envelope (a Bounding Solid of its contents): a stand-in for checks, never
    /// one of the bodies a feature works on unless the feature names it (not by `*`, nor by
    /// naming none).
    pub envelope: bool,
    /// [`Body::retained_bytes`] of `body`, measured when a feature made or changed it (0: not
    /// measured), so the evaluation cache's budget does not walk every body after every feature.
    bytes: usize,
    /// In a world state: the [`ModelBody::geometry_key`] of the model body this one places (0: none).
    source_key: u64,
    /// [`Body::outline_bounds`] of `body` (with the body it was measured on), worked out once
    /// and shared by clones.
    outline: Arc<OnceLock<(Body, solidcraft_geom::Aabb3)>>,
    /// [`ModelBody::volume`] (with the body it was measured on), measured once and shared by clones.
    volume: Arc<OnceLock<(Body, Option<f64>)>>,
    /// [`kernel::measure`] of `body` (with the body it was measured on), worked out once and
    /// shared by clones: inspecting a design again measures only the bodies that changed.
    measured: Arc<OnceLock<(Body, kernel::Result<kernel::BodyMeasure>)>>,
    /// Whether `body` is open (with the body it was measured on): a B-rep one a surface body
    /// ([`Body::is_surface`]), a mesh one with a side only one triangle has
    /// ([`Body::is_closed_mesh`]); worked out once and shared by clones.
    surface: Arc<OnceLock<(Body, bool)>>,
    /// [`ModelBody::iso_lines`] worked out so far, shared by clones.
    iso: Arc<std::sync::Mutex<IsoLines>>,
    /// What cutting it by tools of a recipe gave (see [`ModelBody::cut`]), the latest first;
    /// shared like the mesh, so every unchanged copy of it in later models knows them too.
    cuts: Arc<std::sync::Mutex<Vec<Cut>>>,
    /// What a section view's plane left of it (see [`ModelBody::section`]), the latest first;
    /// shared like the mesh.
    sections: Arc<std::sync::Mutex<Vec<(u64, Option<ModelBody>)>>>,
}

/// A cut remembered by a body: the recipe of the tools and the body's name (its faces are
/// named after it) and what was left.
#[derive(Clone, Debug)]
struct Cut {
    key: u64,
    left: Option<Traced>,
}

/// Cuts a body remembers.
const CUTS_KEPT: usize = 2;

/// Sections a body remembers: a drawing's section views and the one being moved.
const SECTIONS_KEPT: usize = 2;

impl ModelBody {
    /// This body, an operand of the feature being evaluated, cut (`bop`) by `tools` in turn;
    /// `None` when nothing is left. A body of the model before the feature, cut by tools that all
    /// have a recipe, remembers what that gave: a later
    /// evaluation cutting it the same way takes that instead of cutting again.
    fn cut(&self, tools: &[Traced], bop: BoolOp) -> Result<Option<Traced>> {
        let key = recipe_of(tools).filter(|_| matches!(self.made, crate::lineage::Made::Before)).map(|recipe| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (recipe, bop == BoolOp::Cut, &self.name).hash(&mut h);
            h.finish()
        });
        let known = key.and_then(|k| self.cuts.lock().ok().and_then(|cuts| cuts.iter().find(|c| c.key == k).cloned()));
        if let Some(c) = known {
            return Ok(c.left);
        }
        let mut left = Some(self.traced());
        for t in tools {
            left = match left {
                Some(a) => kernel::boolean(&a.body, &t.body, bop)?.map(|r| Traced::step(r, &[&a, t])),
                None => None,
            };
        }
        if let Some(key) = key
            && let Ok(mut cuts) = self.cuts.lock()
        {
            cuts.retain(|c| c.key != key);
            cuts.insert(0, Cut { key, left: left.clone() });
            cuts.truncate(CUTS_KEPT);
        }
        Ok(left)
    }

    /// What a section view draws of this body: the side of `plane` its normal points to (`None`
    /// when nothing is there), named and placed as this body is. A body with a geometry key
    /// remembers what each plane gave, so every unchanged copy of it is cut once per plane. The
    /// section has no key of its own, so no other body shares its geometry.
    pub fn section(&self, plane: &Plane) -> Result<Option<ModelBody>> {
        let key = self.geometry_key().map(|geometry| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (geometry, &self.name).hash(&mut h);
            [plane.origin, plane.x, plane.y].iter().flat_map(|v| [v.x, v.y, v.z]).for_each(|v| v.to_bits().hash(&mut h));
            h.finish()
        });
        let known = key.and_then(|k| self.sections.lock().ok().and_then(|s| s.iter().find(|(at, _)| *at == k).map(|(_, kept)| kept.clone())));
        if let Some(kept) = known {
            return Ok(kept);
        }
        let (kept, _) = kernel::half_space(&self.body, plane, true)?;
        let section = kept.map(|body| ModelBody {
            source_name: self.source_name.clone(),
            source_transform: self.source_transform,
            component: self.component,
            transferred: self.transferred,
            imported: self.imported.clone(),
            ..ModelBody::new(self.name.clone(), body, self.feature)
        });
        if let Some(key) = key
            && let Ok(mut sections) = self.sections.lock()
        {
            sections.retain(|(at, _)| *at != key);
            sections.insert(0, (key, section.clone()));
            sections.truncate(SECTIONS_KEPT);
        }
        Ok(section)
    }

    /// A newly evaluated body with empty caches; a constructor adds its provenance afterward.
    pub fn new(name: String, body: Body, feature: u64) -> Self {
        ModelBody {
            name,
            source_name: None,
            source_transform: crate::IDENTITY,
            body,
            feature,
            frame_member_line: None,
            mesh: Arc::new(OnceLock::new()),
            bounds: Arc::new(OnceLock::new()),
            joint_faces: Arc::new(OnceLock::new()),
            faces: Default::default(),
            mesh_placement: None,
            cam_mesh: Arc::new(OnceLock::new()),
            mirrored_mesh: None,
            opposite_hand: Arc::new(OnceLock::new()),
            plan: Arc::new(Default::default()),
            gpu_plan: Arc::new(Default::default()),
            plan_mesh: Arc::new(Default::default()),
            key: 0,
            names: Default::default(),
            made: Default::default(),
            sketch_frames: None,
            component: 0,
            transferred: false,
            split_from: None,
            imported: None,
            envelope: false,
            bytes: 0,
            source_key: 0,
            outline: Arc::new(OnceLock::new()),
            volume: Arc::new(OnceLock::new()),
            measured: Arc::new(OnceLock::new()),
            surface: Arc::new(OnceLock::new()),
            iso: Default::default(),
            cuts: Default::default(),
            sections: Default::default(),
        }
    }

    /// The body's volume in mm³ ([`kernel::measure_mass`]), measured once and shared by clones: a
    /// body's measure depends on nothing else, so the bill of materials and the physical
    /// properties weigh it once however often they are asked. `None` when it can't be measured.
    pub fn volume(&self) -> Option<f64> {
        let measure = || kernel::measure_mass(&self.body).ok().map(|m| m.volume);
        match self.volume.get_or_init(|| (self.body.clone(), measure())) {
            (measured, v) if measured.same(&self.body) => *v,
            // The body was replaced in this cell's holder since: measure it again.
            _ => measure(),
        }
    }

    /// Whether a feature `f` that names no bodies works on this one: a body of its component
    /// (not the component's envelope), or one `f` made itself.
    pub fn in_default_scope(&self, f: &Feature) -> bool {
        (self.component == f.component && !self.envelope) || self.feature == f.id
    }

    /// The frame a sketch on flat face `face` is drawn in, where what built the body gives the
    /// face one (a sheet made from surfaces: its flat pattern's, so a sketch on a panel measures
    /// as the flat pattern does): the one central hook a body's own frames come in by (#7990).
    /// None where it gives none, or the frame no longer lies on the face as the body has it (the
    /// body was moved since): the face's frame is then the model's.
    pub fn sketch_frame(&self, face: usize) -> Option<Plane> {
        let frame = *self.sketch_frames.as_ref()?.get(face)?.as_ref()?;
        let exact = self.body.planar_face(face)?;
        let tol = 1e-7 * (1.0 + self.body.size());
        (frame.normal().dot(exact.normal()) >= 1.0 - 1e-9 && (frame.origin - exact.origin).dot(exact.normal()).abs() <= tol).then_some(frame)
    }

    /// A surface body: some shell does not close ([`Body::is_surface`]), worked out once.
    pub fn is_surface(&self) -> bool {
        !self.body.is_mesh() && self.is_open()
    }

    /// The body encloses a volume: a solid whose shells all close ([`ModelBody::is_surface`]),
    /// or a mesh body every side of which two triangles share ([`Body::is_closed_mesh`]).
    /// Worked out once.
    pub fn is_closed(&self) -> bool {
        !self.is_open()
    }

    fn is_open(&self) -> bool {
        let open = |b: &Body| if b.is_mesh() { !b.is_closed_mesh() } else { b.is_surface() };
        match self.surface.get_or_init(|| (self.body.clone(), open(&self.body))) {
            (measured, s) if measured.same(&self.body) => *s,
            // The body was replaced in this cell's holder since: look again.
            _ => open(&self.body),
        }
    }

    /// Face measurements for joint origins, at their original chord tolerance. Pose changes
    /// and scratch copies share them; a geometry edit makes a new body and cache.
    pub fn joint_faces(&self) -> Result<Arc<[kernel::FaceInfo]>> {
        let measure = || {
            #[cfg(test)]
            cost_tests::count(|c| c.joint_faces += 1);
            let tol = (self.body.size() * 2e-3).max(1e-3);
            self.body.faces(tol).map(Arc::from)
        };
        let (body, faces) = self.joint_faces.get_or_init(|| (self.body.clone(), measure()));
        if body.same(&self.body) { faces.clone().map_err(Into::into) } else { measure().map_err(Into::into) }
    }

    /// Face measurements ([`Body::faces`]) at chord tolerance `tol`, worked out once for this
    /// geometry and tolerance. `Body::faces` meshes the whole body, and the viewport asks which
    /// faces a feature changed at every rebuild of its highlight (every hover change): a picked
    /// Move feature of a large assembly measured each moved body again each time, for seconds.
    ///
    /// A measurement depends on the body and the tolerance alone. Snapshots and scratch copies
    /// of this geometry share it; a geometry edit makes a new body, which is measured anew. The
    /// last [`FACES_KEPT`] are kept. A failure is kept too: a body never changes in place, so
    /// the same body fails the same way each time.
    pub fn faces_at(&self, tol: f64) -> kernel::Result<Arc<[kernel::FaceInfo]>> {
        let key = tol.to_bits();
        let same = |k: &u64, body: &Body| *k == key && body.same(&self.body);
        let known = self.faces.lock().ok().and_then(|list| list.iter().find(|(k, body, _)| same(k, body)).map(|(_, _, found)| found.clone()));
        if let Some(found) = known {
            return found;
        }
        #[cfg(test)]
        cost_tests::count(|c| c.faces_at += 1);
        let found: kernel::Result<Arc<[kernel::FaceInfo]>> = self.body.faces(tol).map(Arc::from);
        if let Ok(mut list) = self.faces.lock() {
            list.retain(|(k, body, _)| !same(k, body));
            list.insert(0, (key, self.body.clone(), found.clone()));
            list.truncate(FACES_KEPT);
        }
        found
    }

    /// A box around the body without meshing it ([`Body::outline_bounds`]), worked out once.
    pub fn outline_bounds(&self) -> solidcraft_geom::Aabb3 {
        match self.outline.get_or_init(|| (self.body.clone(), self.body.outline_bounds())) {
            (measured, b) if measured.same(&self.body) => *b,
            // The body was replaced in this cell's holder since: measure it again.
            _ => self.body.outline_bounds(),
        }
    }

    /// [`kernel::measure`] of the body (volume, area, centre of mass, box and topology), worked
    /// out once: snapshots and passed-through copies of this geometry share it.
    pub fn measure(&self) -> kernel::Result<kernel::BodyMeasure> {
        let measure = || {
            #[cfg(test)]
            cost_tests::count(|c| c.measures += 1);
            kernel::measure(&self.body)
        };
        let (body, m) = self.measured.get_or_init(|| (self.body.clone(), measure()));
        // The body was replaced in this cell's holder since: measure it again.
        if body.same(&self.body) { m.clone() } else { measure() }
    }

    /// Identity of this body's geometry where it stands: the same for a body evaluated or
    /// placed again from the same inputs at the same place, so work measured on it can be
    /// reused (none for a body the evaluation cache does not key).
    pub fn geometry_key(&self) -> Option<u64> {
        use std::hash::{Hash, Hasher};
        let key = if self.key != 0 { self.key } else { self.source_key };
        if key == 0 {
            return None;
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut h);
        self.source_transform.iter().flatten().for_each(|v| v.to_bits().hash(&mut h));
        Some(h.finish())
    }
    pub fn definition_name(&self) -> &str {
        self.source_name.as_deref().unwrap_or(&self.name)
    }
    /// For a computed placement (a world state's body of a placed component), the geometry key
    /// of the model body it places: placements of one body share it. 0 for any other body.
    pub fn source_key(&self) -> u64 {
        self.source_key
    }
    /// Display mesh, computed once: chord tolerance 1/1000 of the body size, and no facet of a
    /// curved face or edge turning more than [`kernel::DISPLAY_ANGLE`], so that a bend or a
    /// hole that is small beside the body is still drawn round.
    pub fn mesh(&self) -> Arc<Mesh> {
        // From the reference mesher, not the display plan: the model reads this mesh (holes
        // finding their surface, picks naming their face, sketch planes), and a different
        // triangulation moved those and changed what designs build. A GPU viewport draws the
        // plan ([`ModelBody::display_plan`]).
        if let Some(mesh) = self.mesh.get() {
            return mesh.clone();
        }
        match self.mesh_placement.as_deref() {
            Some(placed) => self.mesh.get_or_init(|| placed.mesh()).clone(),
            None => placed_mesh::reference_mesh(&self.body, &self.mesh, self.display_tol()),
        }
    }

    /// The display edges a Fillet or Chamfer with tangent propagation rounds from display edge
    /// `edge`: `edge` first, then each other edge of the body's exact tangent chain through it
    /// ([`kernel::tangent_chain_edges`], the walk the feature itself takes, which stops where a
    /// concave edge runs on as a convex one). The kernel names the chain by the record's edge
    /// indices, which are the display edges'. `at` is the picked point on `edge`. Just `edge`
    /// when the kernel cannot walk the chain.
    pub fn blend_chain(&self, edge: usize, at: Vec3) -> Vec<usize> {
        let mut out = vec![edge];
        if let Ok(chain) = kernel::tangent_chain_edges(&self.body, &[at]) {
            for i in chain {
                if !out.contains(&i) {
                    out.push(i);
                }
            }
        }
        out
    }

    /// The display edges that are one edge to the user with display edge `edge`, sorted, `edge`
    /// among them: the pieces a seam of a curved surface splits it into, as a sphere's seam
    /// ending on a slot's rim splits that rim's arc in two ([`kernel::whole_edge`], decided from
    /// the exact record). The kernel names them by the record's edge indices, which are the
    /// display edges'. Just `edge` for a mesh body, a seam or an edge the body does not have.
    pub fn whole_edge(&self, edge: usize) -> Vec<usize> {
        kernel::whole_edge(&self.body, edge)
    }

    /// The display plan of this body at the viewport's display quality
    /// ([`crate::display_quality::current`]; at Normal, the same tolerance and angle as
    /// [`ModelBody::mesh`]), made once per level; `None` when the body has no plan (a mesh body,
    /// or one the planner refuses).
    pub fn display_plan(&self) -> Option<Arc<TessPlan>> {
        self.display_plan_at(crate::display_quality::current())
    }

    /// The display plan at a given display quality, made once per level.
    pub fn display_plan_at(&self, q: crate::display_quality::DisplayQuality) -> Option<Arc<TessPlan>> {
        let cell = self.plan.get(q.index())?;
        match self.mesh_placement.as_deref() {
            // A placed part shows its definition's plan, moved (planned once for every placement).
            Some(placed) => cell.get_or_init(|| placed.plan(q, &self.name)).clone(),
            None => placed_mesh::definition_plan(&self.body, cell, q, &self.name),
        }
    }

    /// The display plan at a given display quality in the form the GPU evaluates
    /// ([`crate::gpu_plan`]): made once per level from [`ModelBody::display_plan_at`], the same
    /// allocation on every call.
    pub fn gpu_plan_at(&self, q: crate::display_quality::DisplayQuality) -> Option<Arc<solidcore_tess::TessPlan>> {
        self.gpu_plan.get(q.index())?.get_or_init(|| self.display_plan_at(q).map(|plan| Arc::new(crate::gpu_plan::gpu_plan(&plan)))).clone()
    }

    /// The display plan's mesh, evaluated on the CPU once per level: the same triangles a GPU
    /// viewport evaluates and draws for this body. Highlights over a face drawn from the plan use
    /// it, since the reference mesh ([`ModelBody::mesh`]) triangulates the face differently and
    /// the two would fight at equal depth. `None` when the body has no plan.
    pub fn plan_mesh(&self) -> Option<Arc<Mesh>> {
        self.plan_mesh_at(crate::display_quality::current())
    }

    /// The display plan's mesh at a given display quality.
    pub fn plan_mesh_at(&self, q: crate::display_quality::DisplayQuality) -> Option<Arc<Mesh>> {
        self.plan_mesh
            .get(q.index())?
            .get_or_init(|| match self.mesh_placement.as_deref() {
                Some(placed) => placed.plan_mesh(q, &self.name),
                None => self.display_plan_at(q).and_then(|plan| self.body.mesh_of(&plan).ok()).map(Arc::new),
            })
            .clone()
    }

    /// Identity of this body's display geometry, without meshing it: the cell its mesh is kept
    /// in, shared by every clone of the body (timeline snapshots, world states that place it
    /// unchanged) and, while it is held, by no other body.
    pub fn display_identity(&self) -> Arc<dyn std::any::Any + Send + Sync> {
        self.mesh.clone()
    }

    /// For a rigidly placed copy of another body (a placement of a component's body, a pattern
    /// copy), whose display cells are its definition's moved: the identity of that definition,
    /// the same for every copy of it. `None` for a body with display cells of its own.
    pub fn placed_definition(&self) -> Option<usize> {
        self.mesh_placement.as_deref().map(placed_mesh::PlacedMesh::definition_id)
    }

    /// Identity of the display cells of the solid this body shows: its own, or for a rigidly
    /// placed copy its definition's (shared by every placement of it).
    fn solid_id(&self) -> usize {
        self.mesh_placement.as_deref().map_or(Arc::as_ptr(&self.mesh) as usize, placed_mesh::PlacedMesh::definition_id)
    }

    /// How this body is drawn moved from `before`: [`crate::IDENTITY`] when it is `before` where
    /// it stood (its meshes and plans the same), the rigid motion between them when it is the same
    /// solid placed elsewhere (a dragged component's body: `before`'s meshes and plans drawn moved
    /// show it; a definition shown where it is defined is its own placement), `None` when it is
    /// another body or its geometry differs.
    pub fn display_moved_from(&self, before: &ModelBody) -> Option<crate::Mat> {
        if self.name != before.name || self.solid_id() != before.solid_id() {
            return None;
        }
        let (now, was) = (self.mesh_placement.as_deref(), before.mesh_placement.as_deref());
        if let (Some(now), Some(was)) = (now, was)
            && now.tolerance.to_bits() != was.tolerance.to_bits()
        {
            return None;
        }
        let at = |p: Option<&placed_mesh::PlacedMesh>| p.map_or(crate::IDENTITY, |p| *p.transform());
        let (now, was) = (at(now), at(was));
        if now == was {
            return Some(crate::IDENTITY);
        }
        Some(crate::mat_mul(&now, &crate::mat_inverse(&was)?))
    }

    /// [`ModelBody::mesh`] as a shared mesh and the move that places it: for a rigid placement
    /// of a definition, the definition's own mesh (the same one for every placement of it) and
    /// the placement; for any other body its own mesh, unmoved. The mesh moved by the matrix is
    /// `mesh()`, so a viewport can draw every placement of a part from one copy of its
    /// triangles, without making the moved copies.
    pub fn mesh_in_place(&self) -> (Arc<Mesh>, crate::Mat) {
        match self.mesh_placement.as_deref() {
            Some(placed) => placed.source_mesh(),
            None => (self.mesh(), crate::IDENTITY),
        }
    }

    /// The box of [`ModelBody::mesh`] without making a placement's moved copy: the shared mesh's
    /// points, moved. Worked out once (the background mesher works it out ahead, so framing a
    /// large design reads it).
    pub fn mesh_bounds(&self) -> solidcraft_geom::Aabb3 {
        *self.bounds.get_or_init(|| {
            if let Some(mesh) = self.cached_mesh() {
                return mesh.bounds();
            }
            let (mesh, m) = self.mesh_in_place();
            if crate::is_identity(&m) {
                return mesh.bounds();
            }
            mesh.positions.iter().fold(solidcraft_geom::Aabb3::EMPTY, |mut b, p| {
                b.add(crate::apply_point(&m, *p));
                b
            })
        })
    }

    /// Whether [`ModelBody::mesh_bounds`] is worked out already.
    pub fn mesh_bounds_made(&self) -> bool {
        self.bounds.get().is_some()
    }

    /// The box of [`ModelBody::mesh`] seen on a plane, in the plane's own coordinates
    /// ([`Plane::to_local`]): a drawing view's extents (W07), again without making a placement's
    /// moved copy.
    pub fn mesh_bounds_in(&self, plane: &Plane) -> solidcraft_geom::Aabb2 {
        let (mesh, m) = match self.cached_mesh() {
            Some(mesh) => (mesh, crate::IDENTITY),
            None => self.mesh_in_place(),
        };
        // The placement folded into each of the plane's axes: a mesh point p lands at
        // p · axis + offset, with no moved copy of the point made first.
        let [c0, c1, c2, t] = m.map(|r| Vec3::new(r[0], r[1], r[2]));
        let offset = t - plane.origin;
        let axis = |a: Vec3| (Vec3::new(c0.dot(a), c1.dot(a), c2.dot(a)), offset.dot(a));
        let ((ax, ox), (ay, oy)) = (axis(plane.x), axis(plane.y));
        mesh.positions.iter().map(|p| Vec2::new(p.dot(ax) + ox, p.dot(ay) + oy)).collect()
    }

    /// Whether [`ModelBody::plan_mesh_in_place_at`] is made already (tried, at least), so asking
    /// for it does not plan now: a placed copy's is its definition's, shared by every copy.
    pub fn plan_mesh_in_place_made(&self, q: crate::display_quality::DisplayQuality) -> bool {
        let cells = match self.mesh_placement.as_deref() {
            Some(placed) => placed.plan_mesh_cells(),
            None => &self.plan_mesh,
        };
        cells.get(q.index()).is_some_and(|cell| cell.get().is_some())
    }

    /// [`ModelBody::plan_mesh_at`] as a shared mesh and the move that places it, as
    /// [`ModelBody::mesh_in_place`] does for the reference mesh.
    pub fn plan_mesh_in_place_at(&self, q: crate::display_quality::DisplayQuality) -> Option<(Arc<Mesh>, crate::Mat)> {
        match self.mesh_placement.as_deref() {
            Some(placed) => placed.source_plan_mesh(q, &self.name),
            None => Some((self.plan_mesh_at(q)?, crate::IDENTITY)),
        }
    }

    /// [`ModelBody::plan_mesh`] if it is made already; otherwise it is made on a worker thread
    /// (once, however often this is asked) and `None` comes back meanwhile, so a caller drawing a
    /// frame (a face highlighted under the pointer) is not held up by a large body's plan
    /// (#3745): the plan mesh of a long modelled thread takes most of a second. The browser
    /// build has no threads: there it is made now.
    pub fn plan_mesh_soon(&self) -> Option<Arc<Mesh>> {
        let q = crate::display_quality::current();
        let cell = self.plan_mesh.get(q.index())?;
        if let Some(mesh) = cell.get() {
            return mesh.clone();
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            static WARMING: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
            let key = Arc::as_ptr(&self.plan_mesh) as usize + q.index();
            let Ok(mut warming) = WARMING.lock() else { return None };
            if warming.contains(&key) {
                return None;
            }
            let body = self.clone();
            let started = kernel::kernel_thread("plan-mesh").spawn(move || {
                let _ = body.plan_mesh_at(q);
                if let Ok(mut warming) = WARMING.lock() {
                    warming.retain(|k| *k != key);
                }
            });
            if started.is_ok() {
                warming.push(key);
                return None;
            }
        }
        self.plan_mesh_at(q)
    }

    /// The display plan in the form the GPU evaluates ([`ModelBody::gpu_plan_at`]) and its mesh
    /// `k` steps finer than `q` (view-dependent refinement, #1320;
    /// see [`crate::display_quality::refinement_for`]). Step 0 is the cached plan at `q`; finer
    /// steps are made on each call and kept by the viewport, which evicts them when it runs over
    /// its budget. `None` when the body has no plan at that step.
    pub fn refined_display(&self, q: crate::display_quality::DisplayQuality, k: u8) -> Option<(Arc<solidcore_tess::TessPlan>, Arc<Mesh>)> {
        if k == 0 {
            return Some((self.gpu_plan_at(q)?, self.plan_mesh_at(q)?));
        }
        let plan = match self
            .body
            .mesh_plan(&solidcraft_kernel::MeshTolerance { chord: q.refined_chord(self.body.size(), k), angle: Some(q.refined_angle(k)) })
        {
            Ok(plan) => plan,
            Err(e) => {
                log::debug!("{}: no display plan refined {k} steps ({e})", self.name);
                return None;
            }
        };
        let mesh = self.body.mesh_of(&plan).ok()?;
        Some((Arc::new(crate::gpu_plan::gpu_plan(&plan)), Arc::new(mesh)))
    }

    /// The chord tolerance of [`ModelBody::mesh`] (mm): 1/1000 of the definition's size, at least
    /// 1 µm. Proper rigid placements retain that tolerance along with the reference triangles.
    pub fn display_tol(&self) -> f64 {
        self.mesh_placement.as_ref().map(|placed| placed.tolerance).unwrap_or_else(|| (self.body.size() * 1e-3).max(1e-3))
    }

    /// Already computed display geometry; saving must not trigger cold tessellation.
    pub fn cached_mesh(&self) -> Option<Arc<Mesh>> {
        self.mesh.get().cloned()
    }

    /// The mesh a cam's profile is read from: the body's edges at the cam's own tolerance (finer
    /// than the display mesh, as the profile follows the body's outline). The cam reads its
    /// profile from it, and it is kept with the body, so that a solve which works the cam's
    /// table out again does not mesh the body again (#3389). `make` meshes the body: the
    /// profile is a decision made on a mesh (`xtask/mesh-decisions.txt`), so the meshing stays
    /// with the cam.
    pub(crate) fn cam_mesh(&self, make: impl FnOnce() -> kernel::Result<Mesh>) -> Result<Arc<Mesh>> {
        if let Some(mesh) = self.cam_mesh.get() {
            return Ok(mesh.clone());
        }
        #[cfg(test)]
        cost_tests::count(|c| c.cam_meshes += 1);
        let mesh = Arc::new(make()?);
        // Another thread may have made it first: the mesh is the same either way.
        Ok(self.cam_mesh.get_or_init(|| mesh).clone())
    }

    /// The isoparametric curves of face `face` for Isocurve Analysis: `count` each way, evenly
    /// spaced across the face (faces on one surface spaced together, so they share them), exact
    /// and cut to its trims ([`kernel::iso_grid`]), their points at the chord
    /// tolerance of the viewport's display quality. Worked out once per face, count and display
    /// quality, and shared by every clone of this body (timeline snapshots, world states). A mesh
    /// body has no surfaces, so none.
    pub fn iso_lines(&self, face: usize, count: usize) -> Result<Arc<[kernel::IsoLine]>> {
        if self.body.is_mesh() {
            return Err(DocError::Invalid(format!("{} is a mesh body: its faces have no surface parameters, so no isoparametric curves", self.name)));
        }
        let quality = crate::display_quality::current();
        let key = (face, count, quality.index());
        let make = || kernel::iso_grid(&self.body, face, count, quality.chord(self.body.size())).map(Arc::from);
        let Ok(mut cache) = self.iso.lock() else { return Ok(make()?) };
        if !cache.0.as_ref().is_some_and(|b| b.same(&self.body)) {
            // A body replaced in this cell's holder since: what was worked out was on another.
            *cache = (Some(self.body.clone()), Default::default());
        }
        if let Some(lines) = cache.1.get(&key) {
            return Ok(lines.clone());
        }
        let lines = make()?;
        cache.1.insert(key, lines.clone());
        Ok(lines)
    }
}

/// A solved sketch in the evaluated model.
#[derive(Clone, Debug)]
pub struct SolvedSketch {
    pub feature: u64,
    pub name: String,
    pub plane: Plane,
    pub sketch: solidcraft_sketch::Sketch,
    pub report: SolveReport,
    pub profiles: Vec<Profile>,
    /// Planar closed loops the 3D sketch curves take part in, in planes of their own (#741).
    pub profiles_3d: Vec<solidcraft_sketch::Profile3D>,
    /// Open and untied ends of its profile curves (where a profile can fail).
    pub ends: solidcraft_sketch::EndReport,
}

/// A constant fillet of `edges`; with [`FilletFit::Reduce`](crate::FilletFit::Reduce), a radius too
/// large for the geometry around them is reduced where it doesn't fit, to the largest that works
/// for each group of edges sharing a face; the radius per edge is returned. With
/// [`FilletFit::Taper`](crate::FilletFit::Taper) it tapers along each edge it doesn't fit. With
/// the exact fit a radius too large is refused, naming the largest that fits ([`with_largest_fit`]).
/// Also the picks whose edges a round overflowing its faces removed whole ([`kernel::fillet_removing`]).
fn fillet_fitted(
    body: &kernel::Body,
    edges: &[Vec3],
    r: f64,
    fit: crate::FilletFit,
    turn: kernel::FilletTurn,
) -> std::result::Result<(kernel::Body, Option<Vec<f64>>, Vec<Vec3>), kernel::KernelError> {
    #[cfg(test)]
    cost_tests::count(|c| c.blends += 1);
    match kernel::fillet_removing(body, edges, r, turn) {
        Ok((b, gone)) => Ok((b, None, gone)),
        // A refusal the kernel could not size names no radius to try, and the search for the
        // largest radius that fits cannot be guided by it: it then halves the radius from the
        // middle down and blends every edge again for each of the sizes it tries. On the design
        // of #4282 (a cube with a hole through each axis, a 2 mm round of every edge) those ten
        // blends of the forty-one edges took 77 s, fifteen times the blend that failed, and they
        // found nothing: the round fails at any radius there. The refusal is reported as the
        // kernel stated it, less a size it names that no round can have ([`without_unusable_fit`]).
        Err(kernel::KernelError::TooLarge(why)) if fit == crate::FilletFit::Exact && kernel::named_fit(&why).is_none() => {
            Err(kernel::KernelError::TooLarge(without_unusable_fit(&why)))
        }
        Err(kernel::KernelError::TooLarge(why)) if fit == crate::FilletFit::Exact => Err({
            let hint = kernel::named_fit(&why);
            #[cfg(test)]
            cost_tests::count(|c| c.fit_searches += 1);
            with_largest_fit(why, "radius", || kernel::fillet_max_why(body, edges, r, turn, hint))
        }),
        Err(kernel::KernelError::TooLarge(why)) if fit == crate::FilletFit::Reduce => match kernel::fillet_max_each(body, edges, r, turn)? {
            // Built at the radius asked for after all (the groups apart): nothing reduced.
            Some((used, b)) => Ok((b, used.iter().any(|u| *u < r).then_some(used), Vec::new())),
            None => Err(kernel::KernelError::TooLarge(why)),
        },
        Err(kernel::KernelError::TooLarge(_)) if fit == crate::FilletFit::Taper => {
            kernel::fillet_tapered(body, edges, r).map(|(b, _)| (b, None, Vec::new()))
        }
        Err(e) => Err(e),
    }
}

/// The warning of a fillet some of whose edges a larger round, overflowing its faces, removed
/// whole: they are gone, so they are not rounded.
fn removed_note(gone: &[Vec3]) -> String {
    let at = |p: &Vec3| format!("({}, {}, {})", show_mm(p.x), show_mm(p.y), show_mm(p.z));
    match gone {
        [one] => format!("the edge at {} was removed by the larger round, so it is not rounded", at(one)),
        _ => format!("the edges at {} were removed by the larger round, so they are not rounded", gone.iter().map(at).collect::<Vec<_>>().join(", ")),
    }
}

/// A blend's picks (per body index, the picks on that body) without those that take a smooth
/// edge (its faces meet tangentially: a seam or split edge, easy to take with a window pick),
/// each left out named in `notes`, and without a body left with none, when a pick on any body
/// remains; else all of them, for the blend to refuse (a single pick, or only smooth edges:
/// #17919).
fn without_smooth(st: &ModelState, groups: Vec<(usize, Vec<Vec3>)>, notes: &mut Vec<String>) -> Result<Vec<(usize, Vec<Vec3>)>> {
    if groups.iter().map(|(_, picks)| picks.len()).sum::<usize>() < 2 {
        return Ok(groups);
    }
    let mut smooth = Vec::with_capacity(groups.len());
    for (ti, picks) in &groups {
        let mb = st.bodies.get(*ti).ok_or_else(|| DocError::Invalid("body".into()))?;
        smooth.push(kernel::smooth_picks(&mb.body, picks)?);
    }
    if smooth.iter().flatten().all(|s| *s) || !smooth.iter().flatten().any(|s| *s) {
        return Ok(groups);
    }
    let mut at = Vec::new();
    let mut kept = Vec::with_capacity(groups.len());
    for ((ti, picks), smooth) in groups.into_iter().zip(smooth) {
        let (skipped, rest): (Vec<_>, Vec<_>) = picks.into_iter().zip(smooth).partition(|(_, s)| *s);
        at.extend(skipped.iter().map(|(p, _)| format!("({}, {}, {})", show_mm(p.x), show_mm(p.y), show_mm(p.z))));
        if !rest.is_empty() {
            kept.push((ti, rest.into_iter().map(|(p, _)| p).collect()));
        }
    }
    notes.push(match at.as_slice() {
        [one] => format!("1 edge skipped: its faces meet tangentially, so there is nothing to blend (the edge at {one})"),
        _ => format!("{} edges skipped: their faces meet tangentially, so there is nothing to blend (the edges at {})", at.len(), at.join(", ")),
    });
    Ok(kept)
}

thread_local! {
    /// Current feature evaluation validation mode (`EachStep`, `EachFeature`, or `Off`).
    static EVAL_VALIDATION: std::cell::Cell<kernel::Validation> = const { std::cell::Cell::new(kernel::Validation::EachFeature) };
    /// Latest successful thread output per body, confined to the current feature evaluation.
    static CHECKED_THREAD_OUTPUTS: std::cell::Cell<Option<BTreeMap<String, Body>>> = const { std::cell::Cell::new(None) };
}

/// Restores the validation mode and the enclosing feature's checks on scope exit.
struct EvalValidationGuard {
    mode: kernel::Validation,
    checked: Option<BTreeMap<String, Body>>,
}

impl EvalValidationGuard {
    fn new(mode: kernel::Validation) -> Self {
        Self { mode: EVAL_VALIDATION.replace(mode), checked: CHECKED_THREAD_OUTPUTS.replace(Some(BTreeMap::new())) }
    }
}

impl Drop for EvalValidationGuard {
    fn drop(&mut self) {
        CHECKED_THREAD_OUTPUTS.set(self.checked.take());
        EVAL_VALIDATION.set(self.mode);
    }
}

/// Record only an unchanged final thread output after its native check succeeded.
fn remember_checked_thread_output(name: &str, body: &Body) {
    if EVAL_VALIDATION.get() != kernel::Validation::EachFeature {
        return;
    }
    CHECKED_THREAD_OUTPUTS.with(|cell| {
        let mut checked = cell.take();
        if let Some(bodies) = checked.as_mut() {
            bodies.insert(name.to_owned(), body.clone());
        }
        cell.set(checked);
    });
}

/// A successful thread check can replace only the same feature's check of that exact shape.
fn checked_thread_output(body: &Body) -> bool {
    if EVAL_VALIDATION.get() != kernel::Validation::EachFeature {
        return false;
    }
    CHECKED_THREAD_OUTPUTS.with(|cell| {
        let checked = cell.take();
        let found = checked.as_ref().is_some_and(|bodies| bodies.values().any(|old| old.same_shape(body)));
        cell.set(checked);
        found
    })
}

/// Run a full check, counting the calls separately from reused successful thread checks.
fn check_feature_body(body: &Body) -> kernel::Result<()> {
    #[cfg(test)]
    cost_tests::count(|c| c.kept_checks += 1);
    kernel::check_kept(body)
}

/// A blend's refusal as too large (`why`) with the largest `size` (radius, distance) that fits,
/// as `search` finds it by building candidates ([`kernel::fillet_max_why`]). A size is named only
/// when a candidate built at it: solidcore's own estimate (in `why`) is replaced by the size the
/// search built, or, when nothing smaller builds, removed and replaced by what the search met
/// (the smallest candidate's own failure). An estimate no build
/// confirmed sent ladders the wrong way: it could name a size that is itself refused.
fn with_largest_fit(
    why: String,
    size: &str,
    search: impl FnOnce() -> std::result::Result<kernel::FitSearch, kernel::KernelError>,
) -> kernel::KernelError {
    if EVAL_VALIDATION.get() == kernel::Validation::Off {
        return kernel::KernelError::TooLarge(without_unusable_fit(&why));
    }
    match search() {
        Ok(kernel::FitSearch { found: Some((largest, _)), .. }) if kernel::names_largest_fit(&why) => {
            kernel::KernelError::TooLarge(kernel::with_fit_note(&why, size, largest))
        }
        Ok(kernel::FitSearch { found: Some((largest, _)), .. }) => {
            kernel::KernelError::TooLarge(format!("{why}; {}", kernel::largest_fit_note(size, largest)))
        }
        // The search samples sizes and building is not monotone in the size, so it says what it
        // tried, not that nothing smaller builds.
        Ok(kernel::FitSearch { found: None, smallest_refused: Some((at, e)), tried, .. }) => kernel::KernelError::TooLarge(format!(
            "{}; none of the {tried} sizes tried builds (the smallest, {} mm: {})",
            kernel::without_fit_note(&why),
            show_mm(at),
            kernel::without_fit_note(&e.to_string())
        )),
        Ok(kernel::FitSearch { found: None, smallest_refused: None, .. }) => kernel::KernelError::TooLarge(kernel::without_fit_note(&why)),
        Err(e) => {
            log::debug!("the largest {size} that fits: {e}");
            kernel::KernelError::TooLarge(format!("{}; a smaller {size} fails otherwise: {e}", kernel::without_fit_note(&why)))
        }
    }
}

/// `why` without the largest size that fits it names when that size is not one a blend can
/// have ([`kernel::named_fit`] takes none from it): solidcore can refuse a round as too large
/// and name 0 mm as the largest that fits, which no round is. `why` itself otherwise.
fn without_unusable_fit(why: &str) -> String {
    if kernel::names_largest_fit(why) && kernel::named_fit(why).is_none() { kernel::without_fit_note(why) } else { why.to_owned() }
}

/// The warning of a fillet whose radius `asked` was reduced to `used` (per edge).
fn reduced_note(used: &[f64], asked: f64) -> String {
    let mut values: Vec<f64> = used.iter().copied().filter(|r| *r < asked).collect();
    values.sort_by(f64::total_cmp);
    values.dedup();
    let shown = values.iter().map(|v| show_mm(*v)).collect::<Vec<_>>().join(", ");
    let n = used.iter().filter(|r| **r < asked).count();
    if n == used.len() && values.len() == 1 {
        format!("the radius is reduced to {shown} mm, the largest that fits these edges")
    } else {
        format!("the radius is reduced to {shown} mm on {n} of {} edges, the largest that fits them", used.len())
    }
}

/// The warning of a fillet that tapered on some of its edges (a taper per edge, None where the
/// radius is constant), naming the smallest radius a taper ends at.
fn tapered_note(tapers: &[Option<kernel::Taper>]) -> Option<String> {
    let ends: Vec<f64> = tapers.iter().flatten().flat_map(|t| [t.spline.radii.first(), t.spline.radii.last()]).flatten().copied().collect();
    let low = ends.into_iter().reduce(f64::min)?;
    let n = tapers.iter().flatten().count();
    Some(format!("the radius tapers to the room on {n} of {} edges, down to {} mm", tapers.len(), show_mm(low)))
}

/// Blend `edges` one at a time with `blend`, each blend's lineage over the one before: the
/// result and the lineage of what the last blend started from.
fn one_by_one(input: Traced, edges: &[Vec3], blend: impl Fn(&kernel::Body, Vec3) -> kernel::Result<kernel::Body>) -> Result<(kernel::Body, Traced)> {
    let mut b = input;
    let mut from = None;
    for p in edges {
        let next = Traced::step(blend(&b.body, *p)?, &[&b]);
        from = Some(std::mem::replace(&mut b, next));
    }
    match from {
        Some(prev) => Ok((b.body, prev)),
        None => Err(DocError::Invalid("no edges to fillet".into())),
    }
}

/// `Posts` for `Posts (1)`: the body a split piece was numbered after.
fn split_piece_of(name: &str) -> Option<&str> {
    let (base, n) = name.strip_suffix(')')?.rsplit_once(" (")?;
    (!n.starts_with('0') && n.parse::<u32>().is_ok()).then_some(base)
}

/// The warning of a sketch whose profile search was cut short (#6896).
fn profiles_cut_short_note() -> String {
    format!("some profiles may be missing: a group of touching curves is cut into more than {} pieces", solidcraft_sketch::MAX_PROFILE_EDGES)
}

/// Add `note` to a feature's warning.
fn append_warning(warning: &mut Option<String>, note: String) {
    *warning = Some(match warning.take() {
        Some(w) => format!("{w}; {note}"),
        None => note,
    });
}

/// The refusal of a sketch that makes no closed profile, saying where it is open: how many of its
/// profile curves' ends touch nothing, and where the first of them are (two ends a hair apart
/// show a rounding gap a coincident constraint closes), so the curves to join are found without
/// hunting. Positions are in the sketch's own coordinates. No geometry is computed here: the
/// ends are the sketch's own report ([`SolvedSketch::ends`]).
pub(crate) fn no_closed_profiles(ss: &SolvedSketch) -> DocError {
    const NAMED: usize = 4;
    // (a coordinate a hair below zero reads 0, not -0)
    let coordinate = |v: f64| if v.abs() < 5e-7 { "0".to_string() } else { show_mm(v) };
    let named: Vec<String> =
        ss.ends.open.iter().filter_map(|&p| ss.sketch.point(p)).take(NAMED).map(|v| format!("({}, {})", coordinate(v.x), coordinate(v.y))).collect();
    let count = ss.ends.open.len();
    let at = match named.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [first @ .., last] if count <= named.len() => format!("{} and {last}", first.join(", ")),
        all => format!("{} and {} more", all.join(", "), count - all.len()),
    };
    let where_open = match (named.len(), count) {
        (0, _) => String::new(),
        (_, 1) => {
            format!(": an end of its curves touches no other curve, at {at} in the sketch (after sketch.edit, sketch.loose_ends lists the open ends)")
        }
        _ => format!(
            ": {count} ends of its curves touch no other curve, at {at} in the sketch (after sketch.edit, sketch.loose_ends lists them and sketch.constraint.coincident joins two ends)"
        ),
    };
    DocError::Invalid(format!("{} has no closed profiles{where_open}", ss.name))
}

/// A length in mm, without trailing zeros.
fn show_mm(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

impl ModelState {
    /// The smallest radius fillet `feature` used when it reduced a radius too large for its edges.
    pub fn fitted_radius(&self, feature: u64) -> Option<f64> {
        self.fitted_range(feature).map(|(lo, _)| lo)
    }

    /// The smallest and largest radius fillet `feature` used when it reduced a radius too large
    /// for some of its edges.
    pub fn fitted_range(&self, feature: u64) -> Option<(f64, f64)> {
        self.fitted.get(&feature)?.iter().fold(None, |acc, r| Some(acc.map_or((*r, *r), |(lo, hi): (f64, f64)| (lo.min(*r), hi.max(*r)))))
    }

    /// How the pieces join `feature` split off into bodies of their own touched the others, when
    /// it split any off.
    pub fn kept_separate(&self, feature: u64) -> Option<KeptSeparate> {
        self.kept_separate.get(&feature).copied()
    }

    /// The sources (indices into its `bodies`) body Pattern `feature` could not copy and left out.
    pub fn pattern_left_out(&self, feature: u64) -> &[usize] {
        self.pattern_left_out.get(&feature).map_or(&[], Vec::as_slice)
    }

    /// Each edge fillet `feature` tapered (a point on it) with the radius spline it was given.
    pub fn tapered(&self, feature: u64) -> Option<&[(Vec3, kernel::Taper)]> {
        self.tapered.get(&feature).map(Vec::as_slice)
    }

    /// Each edge fillet `feature` blended (a point on it) with the radius it used, when it
    /// reduced a radius too large for some of its edges.
    pub fn fitted_edges(&self, feature: u64) -> Option<Vec<(Vec3, f64)>> {
        let radii = self.fitted.get(&feature)?;
        let edges = self.blend_edges.get(&feature)?;
        Some(edges.iter().copied().zip(radii.iter().copied()).collect())
    }
}

/// The frames joint origins on a face, an edge or a round face resolved to, kept so that solving
/// the joints again (a drive, a drag) does not search the bodies' triangles again for an origin
/// whose bodies did not change. An entry is keyed by the origin's definition, its component and
/// the identity ([`ModelBody::geometry_key`]) and offset of every body of the component, so a
/// body that changed finds none, and copies of a state may share one.
#[derive(Clone, Debug, Default)]
pub(crate) struct OriginFrames {
    found: Arc<std::sync::Mutex<std::collections::HashMap<(String, u64, Vec<u64>), (Mat, Vec<Body>)>>>,
    /// How many origins were resolved from the bodies (not found here).
    resolved: Arc<std::sync::atomic::AtomicUsize>,
}

impl OriginFrames {
    /// Most frames kept; past it all are dropped, as the contact meshes' cache does.
    const MAX: usize = 1 << 14;

    /// The frame kept for `key`, if the bodies it was found on are still the very same bodies as
    /// `bodies`: `ModelBody::body` is public, so a body replaced in place keeps its
    /// [`ModelBody::geometry_key`] and must not be answered from the old geometry.
    pub(crate) fn get(&self, key: &(String, u64, Vec<u64>), bodies: &[Body]) -> Option<Mat> {
        let found = self.found.lock().ok()?;
        let (frame, kept) = found.get(key)?;
        (kept.len() == bodies.len() && kept.iter().zip(bodies).all(|(a, b)| a.same(b))).then_some(*frame)
    }

    /// Count one origin resolved from the bodies, kept or not.
    pub(crate) fn note_resolved(&self) {
        self.resolved.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn put(&self, key: (String, u64, Vec<u64>), frame: Mat, bodies: Vec<Body>) {
        if let Ok(mut found) = self.found.lock() {
            if found.len() >= Self::MAX {
                found.clear();
            }
            found.insert(key, (frame, bodies));
        }
    }

    /// How many origins have been resolved from the bodies, not taken from what was kept.
    pub(crate) fn resolved(&self) -> usize {
        self.resolved.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Model after some prefix of the timeline.
#[derive(Clone, Debug, Default)]
pub struct ModelState {
    /// Frame placements belong to the evaluator. A weak reference avoids a cycle through
    /// cached bodies' naming origins, which retain earlier model states.
    frame_bodies: std::sync::Weak<std::sync::Mutex<FrameBodies>>,
    /// Joint origins already resolved against bodies of this model (see [`OriginFrames`]).
    pub(crate) origin_frames: OriginFrames,
    /// Cosmetic threads (from Thread features and threaded holes).
    pub threads: Vec<ThreadInfo>,
    pub bodies: Vec<ModelBody>,
    /// Shared between states: a feature that does not change a sketch keeps it as it was.
    pub sketches: Vec<Arc<SolvedSketch>>,
    pub body_counter: usize,
    /// Body names the design used and must not use again (a name a `body.rename` freed, #4957),
    /// and the names stored on the suppressed features so far, so a body made while a feature is
    /// suppressed keeps its name once it is back (#4302). The counter starts over at every
    /// evaluation, so without these a design that is evaluated again hands such a name to the next
    /// body.
    pub reserved_body_names: std::collections::BTreeSet<String>,
    /// Bodies converted to sheet metal (#6748): their flat patterns. Such a
    /// body is the folded sheet.
    pub surface_sheets: Vec<crate::SurfaceSheet>,
    /// Construction planes, axes and points as evaluated (by feature).
    pub construct: Vec<crate::construct::Construct>,
    /// Frame members as made and trimmed so far (`frame.member`, `frame.trim`).
    pub frame_members: Vec<crate::frame::MemberRecord>,
    /// Evaluation-cache keys of the sketches (by sketch feature id).
    sketch_keys: BTreeMap<u64, u64>,
    /// The edges each fillet or chamfer blended, as found in this evaluation (by feature id):
    /// a pattern of the feature repeats these, not the points picked when it was made, which a
    /// parameter change may have moved off the edges.
    blend_edges: BTreeMap<u64, Vec<Vec3>>,
    /// Each feature's face points, as it moved them onto its named faces in this evaluation (by
    /// feature id, in the order of [`crate::FeatureKind::face_points`]): a pattern or mirror of
    /// the feature puts its copies there, where the feature found the faces before it ran. A cut
    /// up to a face it splits leaves no one face of that name for the copies to find (#15054).
    /// Not in the digest (the definitions before a pattern, which its key covers, decide them).
    /// Saved models share these points between states, keeping copies editable after reopening.
    named_faces: BTreeMap<u64, Vec<Vec3>>,
    /// The radius per edge a fillet used when it reduced a radius too large for some of its edges
    /// (by feature id, see `FilletFit::Reduce`).
    fitted: BTreeMap<u64, Vec<f64>>,
    /// The radius spline of each edge a fillet tapered (a point on the edge with it, by feature
    /// id, see `FilletFit::Taper`).
    tapered: BTreeMap<u64, Vec<(Vec3, kernel::Taper)>>,
    /// How the pieces a join split off into bodies of their own touched the others (by feature
    /// id), for its notice.
    kept_separate: BTreeMap<u64, KeptSeparate>,
    /// The sources (indices into its `bodies`) each body Pattern could not copy and left out (by
    /// feature id), so the session keeps the names stored for their copies in their places
    /// ([`crate::Feature::pattern_pins`]).
    pattern_left_out: BTreeMap<u64, Vec<usize>>,
    /// For a feature saved before format 14 that split a body, a digest of the later features'
    /// picks its pieces were named by (by feature id, see [`crate::lineage::order_legacy_lumps`]):
    /// the state stands for the feature only while those picks are the same.
    legacy_picks: BTreeMap<u64, u64>,
    /// Each extrude that cut or intersected a sheet made from surfaces, as drawn (by feature id):
    /// its sketch plane and the interval along the plane's normal it ran over. A pocket it leaves
    /// is as deep as the extrude ran from the panel's face (see `sheet_surface_cuts`).
    cut_extents: BTreeMap<u64, (Plane, (f64, f64))>,
}

/// How a piece a join split off into a body of its own (`Touching::Separate`) touched the
/// pieces it was joined with: the most any such piece shared with another, least first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeptSeparate {
    /// A point.
    AtPoint,
    /// A line or a point their boundaries don't hold (a face tangent to another): found on the
    /// exact geometry, which doesn't say which.
    Touching,
    /// A curve: an edge of both.
    AlongEdge,
}

impl KeptSeparate {
    /// Its id in a feature's result: `at_point`, `touching` or `along_edge`.
    pub fn id(self) -> &'static str {
        match self {
            KeptSeparate::AtPoint => "at_point",
            KeptSeparate::Touching => "touching",
            KeptSeparate::AlongEdge => "along_edge",
        }
    }

    /// What a feature's result says about it.
    pub fn notice(self) -> &'static str {
        match self {
            KeptSeparate::AtPoint => "These bodies only touch at a point, so they were kept as separate bodies.",
            KeptSeparate::Touching => "These bodies only touch along a line or at a point, so they were kept as separate bodies.",
            KeptSeparate::AlongEdge => "These bodies only touch along an edge, so they were kept as separate bodies.",
        }
    }
}

impl ModelState {
    /// This state with `bodies` in place of its own, the rest copied: a picture of some of a
    /// model's bodies (the bodies of a design being opened that are meshed so far) without
    /// copying every body of the model first and then dropping them.
    pub fn with_bodies(&self, bodies: Vec<ModelBody>) -> ModelState {
        // Every field named, so a new one is not left out.
        let ModelState {
            frame_bodies,
            origin_frames,
            threads,
            bodies: _,
            sketches,
            body_counter,
            reserved_body_names,
            surface_sheets,
            construct,
            frame_members,
            sketch_keys,
            blend_edges,
            named_faces,
            fitted,
            tapered,
            kept_separate,
            pattern_left_out,
            legacy_picks,
            cut_extents,
        } = self;
        ModelState {
            frame_bodies: frame_bodies.clone(),
            origin_frames: origin_frames.clone(),
            threads: threads.clone(),
            bodies,
            sketches: sketches.clone(),
            body_counter: *body_counter,
            reserved_body_names: reserved_body_names.clone(),
            surface_sheets: surface_sheets.clone(),
            construct: construct.clone(),
            frame_members: frame_members.clone(),
            sketch_keys: sketch_keys.clone(),
            blend_edges: blend_edges.clone(),
            named_faces: named_faces.clone(),
            fitted: fitted.clone(),
            tapered: tapered.clone(),
            kept_separate: kept_separate.clone(),
            pattern_left_out: pattern_left_out.clone(),
            legacy_picks: legacy_picks.clone(),
            cut_extents: cut_extents.clone(),
        }
    }

    /// How many joint origins have been resolved from the bodies of this model (and its copies),
    /// not taken from the frames kept ([`OriginFrames`]).
    pub fn origin_resolutions(&self) -> usize {
        self.origin_frames.resolved()
    }

    /// `body` moved by `matrix` into a feature's frame. The evaluator that made this state keeps
    /// the moved body, so an unchanged body under an unchanged placement is moved (and meshed)
    /// once rather than on every evaluation.
    pub(crate) fn body_in_frame(&self, body: &ModelBody, matrix: &Mat) -> Result<ModelBody> {
        if crate::is_identity(matrix) {
            return Ok(body.clone());
        }
        let cache = self.frame_bodies.upgrade();
        // Not held while the body is moved: copies of the evaluator may share the cache.
        let reused = cache.as_ref().and_then(|c| c.lock().unwrap_or_else(|e| e.into_inner()).reused(body, matrix));
        let placed = match reused {
            Some(placed) => placed,
            None => {
                let placed = ModelBody::new(body.name.clone(), kernel::transform_matrix(&body.body, *matrix)?, body.feature);
                #[cfg(test)]
                cost_tests::count(|c| c.moved += 1);
                if let Some(c) = &cache {
                    c.lock().unwrap_or_else(|e| e.into_inner()).keep(body, matrix, &placed);
                }
                placed
            }
        };
        Ok(in_frame(body, matrix, &placed))
    }

    /// A digest of everything later features can read: bodies (by content identity), sketches,
    /// the body counter, threads, sheet bodies and construction geometry.
    fn digest(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for b in &self.bodies {
            b.name.hash(&mut h);
            b.key.hash(&mut h);
            b.feature.hash(&mut h);
            b.component.hash(&mut h);
            b.transferred.hash(&mut h);
        }
        for s in &self.sketches {
            s.feature.hash(&mut h);
            self.sketch_keys.get(&s.feature).hash(&mut h);
        }
        self.body_counter.hash(&mut h);
        for n in &self.reserved_body_names {
            n.hash(&mut h);
        }
        for (feature, points) in &self.blend_edges {
            feature.hash(&mut h);
            for p in points {
                [p.x, p.y, p.z].map(f64::to_bits).hash(&mut h);
            }
        }
        for (feature, radii) in &self.fitted {
            feature.hash(&mut h);
            for r in radii {
                r.to_bits().hash(&mut h);
            }
        }
        for (feature, tapers) in &self.tapered {
            feature.hash(&mut h);
            for (p, t) in tapers {
                let s = &t.spline;
                for x in [p.x, p.y, p.z, t.start.x, t.start.y, t.start.z].iter().chain(&s.knots).chain(&s.radii) {
                    x.to_bits().hash(&mut h);
                }
                s.degree.hash(&mut h);
            }
        }
        serde_json::to_string(&self.threads).unwrap_or_default().hash(&mut h);
        serde_json::to_string(&self.surface_sheets).unwrap_or_default().hash(&mut h);
        // Construction axes, planes and points so far: features find them here by name, and a
        // result taken from the cache brings back the ones it was made with, so one moved past
        // a feature must change the key (#9544). A state without any hashes as before.
        if !self.construct.is_empty() {
            serde_json::to_string(&self.construct).unwrap_or_default().hash(&mut h);
        }
        // Designs without frame members retain their existing cache identities.
        if !self.frame_members.is_empty() {
            serde_json::to_string(&self.frame_members).unwrap_or_default().hash(&mut h);
        }
        h.finish()
    }

    /// After a feature evaluated to this state from `before` with cache key `key`: bodies it
    /// made or changed take that key, sketches it made a digest of their solved geometry (see
    /// [`quantize`]); the rest keep theirs.
    /// `f` must have gone through [`naming_feature`]: face naming reads no parameter values.
    fn stamp(&mut self, doc: &Document, before: &Arc<ModelState>, f: &Feature, key: u64, mats: Vec<Mat>) {
        let feature = f.id;
        let depth = crate::naming::depth_after(before);
        let origin = Arc::new(crate::naming::NamingOrigin { feature: f.clone(), before: before.clone(), depth, mats });
        // Pieces a cut left are named after their faces, not the kernel's shell order (unless
        // the feature was saved when they were named in that order).
        if !f.shell_piece_order {
            crate::lineage::order_lumps(&mut self.bodies, &origin);
        } else {
            crate::lineage::order_legacy_lumps(&mut self.bodies, before, doc, f);
        }
        match crate::lineage::legacy_picks_digest(&self.bodies, before, doc, f) {
            0 => self.legacy_picks.remove(&f.id),
            d => self.legacy_picks.insert(f.id, d),
        };
        // The bodies before the feature by name, in their order (names repeat only in damaged
        // states): every lookup below is the first match of a linear search, without walking
        // every earlier body for every body, after every feature.
        let mut by_name: std::collections::HashMap<&str, Vec<&ModelBody>> = std::collections::HashMap::with_capacity(before.bodies.len());
        for old in &before.bodies {
            by_name.entry(old.name.as_str()).or_default().push(old);
        }
        for b in &mut self.bodies {
            let olds: &[&ModelBody] = by_name.get(b.name.as_str()).map(Vec::as_slice).unwrap_or_default();
            if !matches!(f.kind, FeatureKind::BodyTransfer { .. })
                && let Some(old) = olds.iter().find(|old| old.transferred)
            {
                b.transferred = true;
                b.component = old.component;
                b.source_transform = old.source_transform;
            }
            // An in-place operation can replace the solid while keeping its producer.
            if b.frame_member_line.is_none()
                && let Some(old) = olds.iter().find(|old| old.feature == b.feature)
            {
                b.frame_member_line = old.frame_member_line.clone();
            }
            let kept = olds.iter().find(|x| x.body.same(&b.body));
            match kept {
                Some(x) => {
                    b.key = x.key;
                    b.names = x.names.clone();
                    b.bytes = if x.bytes > 0 { x.bytes } else { b.body.retained_bytes() };
                }
                None => {
                    let stamped = made_body_key(key, &b.name);
                    // A body a saved model kept, given back by what its feature read
                    // ([`Model::adopt`]), was stamped with this key when it was made: it keeps the
                    // names it was saved with, as the whole state taken back does, since its record
                    // holds no lineage to trace them from again.
                    let saved = b.key == stamped && b.lineage().is_none() && b.names.is_saved();
                    b.key = stamped;
                    if saved {
                        b.names.saved_at(depth);
                    }
                    if !saved && !matches!(f.kind, FeatureKind::BodyTransfer { .. }) {
                        b.names = crate::naming::NameCell::traced(Some(origin.clone()), b.lineage());
                    }
                    // Faces split on purpose stay split through later features (for export).
                    if b.body.split_lines().is_none()
                        && let Some(lines) = olds.first().and_then(|x| x.body.split_lines())
                    {
                        b.body = b.body.clone().with_split_lines(Some(lines.clone()));
                        b.mirrored_mesh = None;
                        b.mesh_placement = None;
                    }
                    // An imported look outlives the operations on the body.
                    if b.body.paint().is_none()
                        && b.body.color().is_none()
                        && let Some(old) = olds.first()
                    {
                        b.imported = crate::appearance::ImportedLook::of(old);
                    }
                    b.bytes = b.body.retained_bytes();
                }
            }
        }
        // Later features read a sketch's solved geometry: they are keyed by it (to the
        // quantum), so a value set back that solves to the same geometry reuses their results.
        if let Some(s) = self.sketches.iter().find(|s| s.feature == feature) {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            serde_json::to_string(&s.plane).unwrap_or_default().hash(&mut h);
            let mut sketch = s.sketch.clone();
            fingerprint_sketch(&mut sketch);
            serde_json::to_string(&sketch).unwrap_or_default().hash(&mut h);
            s.report.ok().hash(&mut h);
            self.sketch_keys.insert(feature, h.finish());
        }
    }
}

/// The key [`ModelState::stamp`] gives body `name` that a feature evaluated with cache key `key`
/// made or changed.
fn made_body_key(key: u64, name: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (key, name).hash(&mut h);
    h.finish()
}

/// Evaluation results by cache key, shared by a model and its copies (previews, scratch
/// sessions), so a result computed once is reused wherever the same inputs come back.
#[derive(Debug)]
pub struct EvalCache {
    /// The model after a feature, by the whole model it started from ([`EvalKey::whole`]).
    states: Store<CachedEval<Arc<ModelState>>>,
    /// What a feature changed, by what it read ([`EvalKey::reads`]): made again on the model as
    /// it is now.
    changes: Store<CachedEval<Change>>,
    /// The bodies of the states the models sharing this cache evaluated, to let go of their
    /// kernel forms ([`Model::release_left`]).
    noted: release::Noted,
    pub hits: usize,
    /// How many times kernel forms were let go here ([`Model::release_left`] found it due): for
    /// tests and diagnostics, as `hits` is.
    pub release_passes: usize,
}

impl Default for EvalCache {
    fn default() -> Self {
        EvalCache { states: Store::new(CACHE_BYTES), changes: Store::new(CHANGE_BYTES), noted: release::Noted::default(), hits: 0, release_passes: 0 }
    }
}

#[derive(Clone, Debug)]
struct CachedEval<S> {
    error: Option<String>,
    error_code: Option<kernel::ErrorCode>,
    warning: Option<String>,
    import_report: Option<Arc<kernel::ImportReport>>,
    repair: Option<Arc<kernel::RepairReport>>,
    lost_projections: Option<Arc<Vec<crate::LostProjection>>>,
    ms: f64,
    state: S,
}

impl EvalCache {
    /// A cache that keeps `bytes` of whole-model results (always at least the last one): a small
    /// budget stands in for a design whose states are large (0: [`CACHE_BYTES`]).
    pub fn with_budget(bytes: usize) -> Self {
        let mut c = EvalCache::default();
        c.set_budget(bytes);
        c
    }

    /// Changes the byte budget of whole-model results (0: [`CACHE_BYTES`]).
    pub(crate) fn set_budget(&mut self, bytes: usize) {
        self.states.budget = if bytes == 0 { CACHE_BYTES } else { bytes };
    }

    /// For tests: drops every whole-model result kept, as the budget does for the ones it has no
    /// room for. Which result a full cache drops is decided by how long each took to compute, so a
    /// test that needs a state gone says so here and does not leave it to a clock.
    #[doc(hidden)]
    pub fn forget_states(&mut self) {
        self.states = Store::new(self.states.budget);
    }

    /// The byte budget of whole-model results.
    pub fn budget(&self) -> usize {
        self.states.budget
    }

    /// Whole-model results stored.
    pub fn len(&self) -> usize {
        self.states.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.states.map.is_empty()
    }
}

/// The results an evaluation ahead on a copy of a model computed or took from the cache, kept for
/// the run that follows on the model itself (#1989). The evaluation cache is bounded by bytes and
/// charges every entry its whole model, so on a design with large bodies it holds only the last
/// few states: by the time the run came, the cache had dropped most of what was computed ahead,
/// and the run computed it all again, on the window thread. A result made from a partial one
/// ([`EvalKey::reads`]) is kept whole too.
#[derive(Debug, Default)]
pub struct KeptResults(std::sync::Mutex<std::collections::HashMap<u64, CachedEval<Arc<ModelState>>>>);

impl KeptResults {
    /// Results kept.
    pub fn len(&self) -> usize {
        self.0.lock().map_or(0, |m| m.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn get(&self, key: u64) -> Option<CachedEval<Arc<ModelState>>> {
        self.0.lock().ok().and_then(|m| m.get(&key).cloned())
    }

    fn put(&self, key: u64, e: &CachedEval<Arc<ModelState>>) {
        if let Ok(mut m) = self.0.lock()
            && (m.len() < CACHE_ENTRIES || m.contains_key(&key))
        {
            m.insert(key, e.clone());
        }
    }
}

/// The results of another copy of a model offered to its evaluation ([`Model::prime_also`]).
#[derive(Clone, Debug)]
struct Primed {
    /// That copy's context ([`context_digest`] and the rest of what `evaluate` mixes in).
    context: u64,
    results: Arc<std::collections::HashMap<u64, FeatureResult>>,
    /// The pose each of its pose-only features was evaluated at.
    poses: Arc<std::collections::HashMap<u64, u64>>,
}

/// Where a model keeps its results besides the cache ([`Model::keep`]). A copy of the model
/// starts without: it keeps nothing it computes for someone else's run.
#[derive(Debug, Default)]
struct KeptSlot(Option<Arc<KeptResults>>);

impl Clone for KeptSlot {
    fn clone(&self) -> Self {
        KeptSlot(None)
    }
}

/// Results kept (the least worth dropped first, see [`Store`]).
const CACHE_ENTRIES: usize = 4096;

/// Bytes of results kept (the least worth dropped first). A cached result is as big as the model it
/// holds, so bounding the cache by entry count alone let a long editing session keep every
/// superseded state: the entries nothing else refers to any more are exactly the ones a byte
/// budget drops.
const CACHE_BYTES: usize = 64 << 20;

/// Bytes of changes kept. A change holds only the bodies its feature made, so many more are
/// kept than whole models: in a large design one model fills [`CACHE_BYTES`].
const CHANGE_BYTES: usize = 256 << 20;

/// Values by key, kept by what losing them would cost (GreedyDual-Size): past
/// [`CACHE_ENTRIES`] values or `budget` bytes, the one worth least goes first. A value is worth
/// the milliseconds it took to compute per byte it holds, on top of the worth of the last value
/// dropped when it was stored or used, so a costly result outlasts many cheap ones (recomputing
/// those is quick), and values not used for long still give way to new ones.
#[derive(Debug)]
struct Store<V> {
    map: std::collections::HashMap<u64, Stored<V>>,
    /// Keys by worth, the least first (worth as bits: non-negative, so they order as numbers).
    by_worth: std::collections::BTreeSet<(u64, u64)>,
    /// The worth of the last value dropped.
    floor: f64,
    /// What the values cost together.
    bytes: usize,
    budget: usize,
}

#[derive(Debug)]
struct Stored<V> {
    value: V,
    bytes: usize,
    /// Milliseconds it took to compute.
    ms: f64,
    worth: f64,
}

impl<V: Clone> Store<V> {
    fn new(budget: usize) -> Self {
        Store { map: Default::default(), by_worth: Default::default(), floor: 0.0, bytes: 0, budget }
    }

    fn worth(&self, ms: f64, bytes: usize) -> f64 {
        let ms = if ms.is_finite() { ms.max(0.0) } else { 0.0 };
        self.floor + (ms + 0.01) / bytes.max(1) as f64
    }

    /// The value at `key`, which counts as used now.
    fn get(&mut self, key: u64) -> Option<V> {
        let (ms, bytes) = self.map.get(&key).map(|e| (e.ms, e.bytes))?;
        let worth = self.worth(ms, bytes);
        let e = self.map.get_mut(&key)?;
        self.by_worth.remove(&(e.worth.to_bits(), key));
        e.worth = worth;
        self.by_worth.insert((worth.to_bits(), key));
        Some(e.value.clone())
    }

    /// Keep `value`, which took `ms` to compute and holds `bytes`. One value is always kept, so
    /// a single model larger than the budget cannot empty the cache and force a full recompute
    /// on every undo.
    fn put(&mut self, key: u64, value: V, bytes: usize, ms: f64) {
        let worth = self.worth(ms, bytes);
        if let Some(old) = self.map.insert(key, Stored { value, bytes, ms, worth }) {
            self.bytes = self.bytes.saturating_sub(old.bytes);
            self.by_worth.remove(&(old.worth.to_bits(), key));
        }
        self.bytes = self.bytes.saturating_add(bytes);
        self.by_worth.insert((worth.to_bits(), key));
        while self.map.len() > CACHE_ENTRIES || (self.bytes > self.budget && self.map.len() > 1) {
            let Some((bits, k)) = self.by_worth.pop_first() else { break };
            if let Some(old) = self.map.remove(&k) {
                self.bytes = self.bytes.saturating_sub(old.bytes);
                self.floor = f64::from_bits(bits);
            }
        }
    }
}

impl<S> CachedEval<S> {
    /// The same evaluation, with `state` for its result.
    fn with<T>(&self, state: T) -> CachedEval<T> {
        CachedEval {
            error: self.error.clone(),
            error_code: self.error_code.clone(),
            warning: self.warning.clone(),
            import_report: self.import_report.clone(),
            repair: self.repair.clone(),
            lost_projections: self.lost_projections.clone(),
            ms: self.ms,
            state,
        }
    }
}

/// What one evaluation of a feature leaves in the cache: the model after it, by the whole model
/// it started from, and what it changed, by what it read, when that is all it changed.
#[derive(Debug)]
struct CacheEntry {
    key: u64,
    whole: CachedEval<Arc<ModelState>>,
    bytes: usize,
    change: Option<(u64, CachedEval<Change>)>,
}

impl CacheEntry {
    /// Stores the entry in `cache`, and its whole result in `kept`.
    fn put(self, cache: &std::sync::Mutex<EvalCache>, kept: &KeptSlot) {
        if let Some(k) = &kept.0 {
            k.put(self.key, &self.whole);
        }
        if let Ok(mut m) = cache.lock() {
            let ms = self.whole.ms;
            if let Some((k, change)) = self.change {
                let bytes = change.state.retained_bytes();
                m.changes.put(k, change, bytes, ms);
            }
            m.states.put(self.key, self.whole, self.bytes, ms);
        }
    }
}

impl EvalCache {
    fn state(&mut self, key: u64) -> Option<CachedEval<Arc<ModelState>>> {
        let r = self.states.get(key);
        self.hits += usize::from(r.is_some());
        r
    }
    fn change(&mut self, key: u64) -> Option<CachedEval<Change>> {
        let r = self.changes.get(key);
        self.hits += usize::from(r.is_some());
        r
    }
}

impl SolvedSketch {
    /// The curves `ids` in order along the open chain they make, from one free end to the other,
    /// whatever order they were given in. Refused unless they join end to end into one open chain.
    pub fn chain_order(&self, ids: &[String]) -> Result<Vec<String>> {
        let tol = 1e-5;
        let mut ends = Vec::with_capacity(ids.len());
        for id in ids {
            let ci = self.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("curve `{id}`")))?;
            let segs = self.sketch.segs(ci);
            let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return Err(DocError::Invalid(format!("curve `{id}` has no length"))) };
            ends.push((id.clone(), first.start(), last.end()));
        }
        let joined = |p: Vec2, skip: usize| ends.iter().enumerate().any(|(k, e)| k != skip && (e.1.dist(p) < tol || e.2.dist(p) < tol));
        // Start at a curve with a free end, walking away from it.
        let Some((start, at)) = ends.iter().enumerate().find_map(|(k, e)| match (joined(e.1, k), joined(e.2, k)) {
            (false, _) => Some((k, e.2)),
            (_, false) => Some((k, e.1)),
            _ => None,
        }) else {
            return Err(DocError::Invalid("the curves close into a loop: an open chain is needed".into()));
        };
        let (mut order, mut at) = (vec![start], at);
        while order.len() < ends.len() {
            let Some((k, e)) = ends.iter().enumerate().find(|(k, e)| !order.contains(k) && (e.1.dist(at) < tol || e.2.dist(at) < tol)) else {
                return Err(DocError::Invalid("the curves are not joined end to end into one chain".into()));
            };
            at = if e.1.dist(at) < tol { e.2 } else { e.1 };
            order.push(k);
        }
        Ok(order.iter().filter_map(|k| ends.get(*k).map(|e| e.0.clone())).collect())
    }

    /// Rough heap footprint (see [`ModelState::retained_bytes`]).
    pub fn retained_bytes(&self) -> usize {
        let entities = self.sketch.points.len() + self.sketch.curves.len() + self.sketch.constraints.len() + self.sketch.links.len();
        let loops: usize = self.profiles.iter().map(|p| p.region.outer.segs.len() + p.region.holes.iter().map(|h| h.segs.len()).sum::<usize>()).sum();
        entities * 192 + loops * 64 + 512
    }
}

impl ModelState {
    /// Rough heap footprint, for the evaluation cache's byte budget. The bodies' B-rep
    /// topology is what makes a kept timeline snapshot expensive, so it dominates; sketches
    /// and the rest are counted from their entity counts.
    pub fn retained_bytes(&self) -> usize {
        let bodies: usize = self.bodies.iter().map(|b| if b.bytes > 0 { b.bytes } else { b.body.retained_bytes() }).sum();
        let sketches: usize = self.sketches.iter().map(|s| s.retained_bytes()).sum();
        bodies + sketches + self.surface_sheets.len() * 4096 + self.threads.len() * 192 + self.construct.len() * 192
    }

    /// [`ModelState::retained_bytes`] of what this state holds that `before` does not: bodies
    /// and sketches it shares with `before` are already paid for by the cached state that made
    /// them. Charging every cached state its whole model let a design whose model is large next
    /// to the budget keep only its last few states, so setting a parameter back recomputed what
    /// had dropped out.
    fn added_bytes(&self, before: &ModelState) -> usize {
        // `before`'s bodies by solid, and its sketches by address: looked up per body and sketch,
        // not searched (every feature's result is charged, so searching was bodies² per feature).
        // Bodies sharing a solid are still told apart by `same_shape` (the mesh as well).
        let mut solids: std::collections::HashMap<usize, Vec<&Body>> = std::collections::HashMap::with_capacity(before.bodies.len());
        for o in &before.bodies {
            solids.entry(o.body.solid_id()).or_default().push(&o.body);
        }
        let bodies: usize = self
            .bodies
            .iter()
            .filter(|b| !solids.get(&b.body.solid_id()).is_some_and(|os| os.iter().any(|o| o.same_shape(&b.body))))
            .map(|b| if b.bytes > 0 { b.bytes } else { b.body.retained_bytes() })
            .sum();
        let kept: std::collections::HashSet<*const SolvedSketch> = before.sketches.iter().map(Arc::as_ptr).collect();
        let sketches: usize = self.sketches.iter().filter(|s| !kept.contains(&Arc::as_ptr(s))).map(|s| s.retained_bytes()).sum();
        // A state holds its sheets made from surfaces as it holds its threads, a copy of its own:
        // each is charged.
        bodies + sketches + self.surface_sheets.len() * 4096 + self.threads.len() * 192 + self.construct.len() * 192
    }

    pub fn body(&self, name: &str) -> Option<&ModelBody> {
        #[cfg(test)]
        cost_tests::count(|c| c.body_searches += 1);
        self.bodies.iter().find(|b| b.name == name)
    }

    /// The frame member whose body is the body called `name` now. A member's record stays when a
    /// later feature removes its body, and another body can take the name; it counts only while
    /// the body of that name is the one its feature made.
    pub fn frame_member(&self, name: &str) -> Option<&crate::frame::MemberRecord> {
        let body = self.body(name)?;
        self.frame_members.iter().rev().find(|m| m.body == name && m.feature == body.feature)
    }

    /// Make every body's display mesh now, on all cores (bodies are meshed independently and
    /// each keeps its own mesh, so the meshes are those `ModelBody::mesh` makes one by one).
    /// Stops starting bodies once `go_on` says no.
    pub fn warm_meshes(&self, go_on: impl Fn() -> bool + Sync) {
        // Only bodies without a mesh: starting threads just to find every mesh made costs more
        // than looking (most bodies are unchanged from one command to the next).
        let cold: Vec<_> = self.bodies.iter().filter(|b| b.mesh.get().is_none()).collect();
        kernel::for_each_on_threads(&cold, 16, |b| {
            if go_on() {
                let _ = b.mesh();
            }
        });
    }

    /// Prepare reference meshes and display plans at the current quality for ahead jobs.
    pub fn warm_display(&self) {
        let q = crate::display_quality::current();
        let planned = |b: &ModelBody| b.plan.get(q.index()).is_none_or(|cell| cell.get().is_some());
        let cold: Vec<_> = self.bodies.iter().filter(|b| b.mesh.get().is_none() || !planned(b)).collect();
        kernel::for_each_on_threads(&cold, 16, |b| {
            let _ = b.mesh();
            let _ = b.display_plan();
        });
    }

    /// Prepare reference picking geometry and the GPU plan/highlight mesh at exactly the
    /// requested quality. The first viewport frame then consumes initialized caches only.
    pub fn warm_display_at(&self, quality: crate::display_quality::DisplayQuality, go_on: impl Fn() -> bool + Sync) {
        kernel::for_each_on_threads(&self.bodies, 16, |body| {
            if go_on() {
                let _ = body.mesh();
            }
            if go_on() {
                let _ = body.plan_mesh_at(quality);
            }
        });
    }
    pub fn sketch(&self, feature: u64) -> Option<&SolvedSketch> {
        self.sketches.iter().find(|s| s.feature == feature).map(|s| &**s)
    }
}

/// Evaluation result of one feature.
#[derive(Clone, Debug, Serialize)]
pub struct FeatureResult {
    pub id: u64,
    pub name: String,
    pub error: Option<String>,
    /// The stable code of the kernel call `error` was made from ([`kernel::failure`]), when it was
    /// made from one: to group and replay failures by, without reading `error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<kernel::ErrorCode>,
    /// `error` is the kernel's internal error (a panic it caught, [`kernel::KernelError::Internal`]),
    /// not the feature's failure: the command that made the feature reports it as one.
    #[serde(skip)]
    pub internal: bool,
    pub warning: Option<String>,
    /// An Import feature's warnings by kind and part, with what came in of the file's parts
    /// (`warning` is the first of them as the kernel words them).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub import_report: Option<Arc<kernel::ImportReport>>,
    /// What a Repair Part feature changed, face by face (the command's reply says it).
    #[serde(skip)]
    pub repair: Option<Arc<kernel::RepairReport>>,
    /// A sketch that failed for projections kept by names that find nothing: which
    /// ([`crate::DocError::LostProjections`]; `error` is their message).
    #[serde(skip)]
    pub lost_projections: Option<Arc<Vec<crate::LostProjection>>>,
    pub ms: f64,
    pub skipped: bool,
    /// Not evaluated on the document's current inputs: a feature at or after a sketch shown live
    /// ([`Model::show_live_sketch`]). It keeps its last result until the next `evaluate`, which
    /// never reuses it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub stale: bool,
    #[serde(skip)]
    fingerprint: u64,
    /// Taken from a saved model that did not keep this feature's state (#3220): `state` only
    /// stands in for it until [`Model::evaluate`] evaluates the feature again, which
    /// [`Model::adopt`] does before the model is used.
    #[serde(skip)]
    unkept: bool,
    #[serde(skip)]
    pub state: Arc<ModelState>,
}

/// Incremental evaluator: holds the state after every feature.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub results: Vec<FeatureResult>,
    /// Imports whose SMLib check found a part to repair after they were shown, not yet taken by
    /// a window ([`Model::take_import_notices`]), in the order their checks were settled.
    import_notices: Vec<u64>,
    /// The import check runs this model has put in `import_notices`, by feature.
    told_imports: Vec<(u64, kernel::ImportChecks)>,
    /// Import heals were settled into the model that no evaluation has brought in yet.
    import_heals_owed: bool,
    pub param_errors: BTreeMap<String, String>,
    empty: Arc<ModelState>,
    /// Features recomputed by the last `evaluate`.
    pub last_recomputed: usize,
    /// Features recomputed by every `evaluate` of this model (and of the model it was copied
    /// from) so far.
    pub recomputed: usize,
    /// Features whose result the last `evaluate` took from the cache instead of recomputing: the
    /// same feature on the same inputs was evaluated before (an edit set back, undo, redo, a
    /// configuration row). Their geometry did change since the previous evaluation.
    pub last_restored: usize,
    /// Results by inputs, shared with this model's copies.
    pub cache: Arc<std::sync::Mutex<EvalCache>>,
    /// Evaluate everything from scratch (no reuse; tests compare the two).
    pub no_cache: bool,
    /// Results kept besides the cache ([`Model::keep`]).
    kept: KeptSlot,
    /// Embedded imports' content digests from the last evaluation.
    import_digests: ImportDigests,
    /// The address of the shared document whose fingerprints the results were last given in
    /// full, with [`fingerprints_digest`] of them then ([`Model::pinned_prefix`]). Only an
    /// address: a reference to the document would make every in-place edit of it a copy.
    fingerprinted: Option<(usize, u64)>,
    /// The document context the last `evaluate` ran in ([`context_digest`]).
    context: u64,
    /// The last active result of each feature hidden by the history marker, with its document
    /// context. These share the existing states, so rolling back does not surrender them to
    /// the bounded cache. A result leaves this map when its feature is active again or removed.
    rolled_results: std::collections::HashMap<u64, (u64, FeatureResult)>,
    /// Results other copies of this model evaluated (ahead, off the window thread, or for
    /// another configuration row) that `evaluate` takes for features they match
    /// ([`Model::prime`], [`Model::prime_also`]), each with that copy's context and the poses
    /// its pose-only features were evaluated at.
    primed: Vec<Primed>,
    /// Mesh bodies from the import reader, offered to the next evaluation only.
    prepared_meshes: Option<(u64, PreparedMeshImport)>,
    /// Shared with copies of this evaluator; states hold only a weak reference.
    frame_bodies: Arc<std::sync::Mutex<FrameBodies>>,
    /// When SMLib's check runs on the features this model evaluates ([`Model::set_validation`]).
    validation: kernel::Validation,
    /// The last `evaluate` ran with the tolerance heal preference off
    /// ([`kernel::heals_tolerance_defects`]; [`Model::heals`]).
    heal_off: bool,
    /// Inputs proven to describe placement-independent geometry, and the state they produced.
    placement_inputs: Option<placement_reuse::Inputs>,
    /// All evaluated features use supported local inputs; computed with the existing owner index.
    placement_local: bool,
    /// A failed sketch shown while it is edited ([`Model::hold_failed_sketch`]).
    held: Option<HeldSketch>,
    /// The [`placement_digest`] each [`pose_only`] feature was last evaluated at, by feature id.
    /// Their fingerprints leave it out, so a pose change doesn't reach the features after them.
    poses: std::collections::HashMap<u64, u64>,
    /// `evaluate` returns without waiting for the SMLib checks of its last features still running
    /// ([`Model::set_background_checks`]).
    background_checks: bool,
    /// The checks the last `evaluate` left running ([`Model::settle_feature_checks`]).
    feature_checks: Option<Arc<overlap::Pending>>,
    /// A check refusal settled after its evaluation returned, for the next `evaluate` to fail the
    /// refused feature with.
    carried_refusal: Option<overlap::Carried>,
    /// A check refusal was settled that no evaluation has brought in yet.
    feature_refusal_owed: bool,
}

/// A failed sketch built for its editor (#12085): `shown` is the final state `base` with that
/// sketch in it. [`Model::state`] gives it only while the results still end in `base`, so the
/// next evaluation, or anything else that changes the results, lets it go.
#[derive(Clone, Debug)]
struct HeldSketch {
    feature: u64,
    base: Arc<ModelState>,
    shown: Arc<ModelState>,
}

/// Parameters a feature uses directly (inputs, sketch dimensions and the expressions of a
/// sketch's texts, the text parameters of their words included), sorted.
fn param_names(f: &Feature) -> Vec<String> {
    let mut names: Vec<String> = f.kind.expressions().iter().flat_map(|e| expr::references(e)).collect();
    if let FeatureKind::Sketch { sketch, .. } = &f.kind {
        names.extend(sketch.dimension_params().into_iter().cloned());
        names.extend(sketch.patterns.iter().flat_map(|p| p.expressions.values()).flat_map(|e| expr::references(e)));
        names.extend(sketch.text_exprs().into_iter().flat_map(|(_, e)| expr::references(e)));
        names.extend(sketch.text_word_exprs().into_iter().flat_map(|e| crate::text_params::references(e)));
    }
    names.sort();
    names.dedup();
    names
}

/// Hash a feature definition, its referenced values and the timeline prefix it evaluates against.
fn fingerprint(doc: &Document, prev: u64, f: &Feature, vals: &BTreeMap<String, Value>, rolled_back: bool, steps: &ImportDigests) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    prev.hash(&mut h);
    rolled_back.hash(&mut h);
    vals.get(DESIGN_LEN).map(|v| v.v.to_bits()).hash(&mut h);
    hash_feature_json(f, steps, &mut h);
    // An import read, and a Repair evaluated, with the tolerance heal off are other results.
    if steps.heal_off && matches!(f.kind, FeatureKind::Import { .. } | FeatureKind::Repair { .. }) {
        ("tolerance heal", "off").hash(&mut h);
    }
    // An import whose checks healed its parts is the healed import ([`ImportDigests::heals`]).
    if let Some(heals) = steps.heals(f.id) {
        heals.digest().hash(&mut h);
    }
    if matches!(f.kind, FeatureKind::FrameMember { .. }) {
        // Older cached member bodies do not have line provenance; rebuild them once.
        "frame-member-line".hash(&mut h);
    }
    for n in param_names(f) {
        n.hash(&mut h);
        match (vals.get(&n), doc.text_param(&n)) {
            (Some(v), _) => {
                v.v.to_bits().hash(&mut h);
                v.len.hash(&mut h);
                v.ang.hash(&mut h);
            }
            (None, Some(p)) => p.text.hash(&mut h),
            (None, None) => 0u8.hash(&mut h),
        }
    }
    h.finish()
}

/// Content digests for embedded imports. Shared, unchanged arrays/text are read once;
/// feature metadata is still fingerprinted on every evaluation. Rebuilding this map prunes
/// removed features and never trusts an address without retaining and checking its Arc.
#[derive(Clone, Debug, Default)]
struct ImportDigests {
    steps: std::collections::HashMap<u64, (Arc<str>, u64)>,
    /// An import kept as records: its records and their digest, computed once per records.
    records: std::collections::HashMap<u64, (Arc<kernel::ImportRecords>, u64)>,
    meshes: std::collections::HashMap<u64, Vec<MeshDigest>>,
    /// The parts SMLib's checks of an import healed once they ended (after the import was
    /// first shown: [`Model::settle_import_checks`]), by import feature, with the digest of the
    /// text they were read from: the import is then its healed bodies, and its fingerprint (and
    /// so every fingerprint after it) says so. Kept while the feature's text is the same.
    heals: std::collections::HashMap<u64, (u64, Arc<kernel::ImportHeals>)>,
    /// Evaluated with the tolerance heal preference off: imports and Repair features say so in
    /// their fingerprints ([`fingerprint`]), so turning the preference either way evaluates them
    /// again. Nothing is added with it on, so the fingerprints saved before it stand.
    heal_off: bool,
}

#[derive(Clone, Debug)]
struct MeshDigest {
    positions: Arc<[f64]>,
    triangles: Arc<[u32]>,
    corner_normals: Arc<[f64]>,
    digest: u64,
}

impl MeshDigest {
    fn matches(&self, mesh: &crate::MeshData) -> bool {
        Arc::ptr_eq(&self.positions, &mesh.positions)
            && Arc::ptr_eq(&self.triangles, &mesh.triangles)
            && Arc::ptr_eq(&self.corner_normals, &mesh.corner_normals)
    }
}

impl ImportDigests {
    fn of(doc: &Document, known: &ImportDigests, heal: bool) -> ImportDigests {
        let mut out = ImportDigests { heal_off: !heal, ..ImportDigests::default() };
        for f in &doc.features {
            match &f.kind {
                FeatureKind::Import { step, records, .. } => {
                    let digest = known.get(f.id, step).unwrap_or_else(|| step_text_digest(step));
                    out.steps.insert(f.id, (step.clone(), digest));
                    // Kept while the heal preference is on: off, the import is its parts as read.
                    if let Some(heals) = known.heals.get(&f.id).filter(|(of, _)| *of == digest && heal) {
                        out.heals.insert(f.id, heals.clone());
                    }
                    if let Some(r) = records {
                        let digest = known.records_digest(f.id, r).unwrap_or_else(|| records_digest(r));
                        out.records.insert(f.id, (r.clone(), digest));
                    }
                }
                FeatureKind::MeshImport { meshes, .. } => {
                    let digests = meshes
                        .iter()
                        .enumerate()
                        .map(|(index, mesh)| {
                            let digest = known.mesh(f.id, index, mesh).unwrap_or_else(|| mesh_content_digest(mesh));
                            MeshDigest {
                                positions: mesh.positions.clone(),
                                triangles: mesh.triangles.clone(),
                                corner_normals: mesh.corner_normals.clone(),
                                digest,
                            }
                        })
                        .collect();
                    out.meshes.insert(f.id, digests);
                }
                _ => {}
            }
        }
        out
    }

    /// The heals the import `id` is evaluated with, if its checks healed its parts.
    fn heals(&self, id: u64) -> Option<&Arc<kernel::ImportHeals>> {
        self.heals.get(&id).map(|(_, heals)| heals)
    }

    fn get(&self, id: u64, step: &Arc<str>) -> Option<u64> {
        self.steps.get(&id).filter(|(text, _)| Arc::ptr_eq(text, step)).map(|(_, digest)| *digest)
    }

    fn records_digest(&self, id: u64, records: &Arc<kernel::ImportRecords>) -> Option<u64> {
        self.records.get(&id).filter(|(r, _)| Arc::ptr_eq(r, records)).map(|(_, digest)| *digest)
    }

    fn mesh(&self, id: u64, index: usize, mesh: &crate::MeshData) -> Option<u64> {
        self.meshes.get(&id)?.get(index).filter(|cached| cached.matches(mesh)).map(|cached| cached.digest)
    }
}

fn mesh_content_digest(mesh: &crate::MeshData) -> u64 {
    #[cfg(test)]
    cost_tests::count(|c| c.mesh_digested += 1);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    mesh.positions.len().hash(&mut h);
    for coordinate in mesh.positions.iter() {
        coordinate.to_bits().hash(&mut h);
    }
    mesh.triangles.hash(&mut h);
    for n in mesh.corner_normals.iter() {
        n.to_bits().hash(&mut h);
    }
    h.finish()
}

/// An import's records digested as their serialised form; records that will not serialise
/// digest as nothing (their evaluation fails and says why).
fn records_digest(records: &kernel::ImportRecords) -> u64 {
    step_text_digest(&kernel::records_text(records).unwrap_or_default())
}

fn step_text_digest(text: &str) -> u64 {
    #[cfg(test)]
    cost_tests::count(|c| c.digested += 1);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// Positions and radii are fingerprinted to this quantum (mm): far below anything a design
/// can show, far above the solver's rounding, so a sketch solved again to the same geometry
/// reads as the same input.
const QUANTUM: f64 = 1e-6;

/// Put what a solve writes into a sketch on the [`QUANTUM`] grid: positions, radii and the
/// measured values of driven dimensions.
fn quantize(sk: &mut solidcraft_sketch::Sketch) {
    let q = |v: f64| (v / QUANTUM).round() * QUANTUM;
    for p in &mut sk.points {
        p.pos = Vec2::new(q(p.pos.x), q(p.pos.y));
    }
    for c in &mut sk.curves {
        if let CurveKind::Circle { r, .. } | CurveKind::Ellipse { r, .. } = &mut c.kind {
            *r = q(*r);
        }
    }
    for c in sk.constraints.iter_mut().filter(|c| c.driven) {
        if let Some(v) = c.kind.value() {
            c.kind.set_value(q(v));
        }
    }
}

/// The stored solve as a cache key: on the [`QUANTUM`] grid, without the editor state
/// (see [`solidcraft_sketch::Sketch::clear_id_counters`]).
fn fingerprint_sketch(sk: &mut solidcraft_sketch::Sketch) {
    quantize(sk);
    sk.clear_id_counters();
}

/// A feature as fingerprinted: its definition, a sketch's stored solve quantized, an imported
/// file's text or mesh payload by its content digest.
/// Hash [`feature_json`] of `f` into `h`. Every evaluation fingerprints every feature, and in a
/// design built command by command almost all of them are the same as last time: the text of a
/// feature is kept (by id) with the feature it was written from, and written again only when the
/// feature differs. Embedded imports also depend on their content digests, so their small
/// metadata-and-digest representation is written each time without comparing large arrays.
///
/// The texts are shared by every thread: a command evaluated ahead runs on a new worker thread
/// each time (the desktop's deferred commands and delete checks, the control channel's ahead
/// jobs), and a per-thread store started empty there, so every ahead run wrote out every
/// feature again. The lock is held only to look up or store an entry, never while a feature is
/// compared or written; an entry is the same whichever thread wrote it, since the text is a
/// function of the feature alone.
fn hash_feature_json(f: &Feature, steps: &ImportDigests, h: &mut impl Hasher) {
    type Written = std::collections::HashMap<u64, Arc<(Feature, String)>>;
    static WRITTEN: std::sync::LazyLock<std::sync::Mutex<Written>> = std::sync::LazyLock::new(Default::default);
    if matches!(f.kind, FeatureKind::Import { .. } | FeatureKind::MeshImport { .. }) {
        feature_json(f, steps).hash(h);
        return;
    }
    let seen = WRITTEN.lock().unwrap_or_else(|e| e.into_inner()).get(&f.id).cloned();
    if let Some(seen) = seen
        && seen.0 == *f
    {
        seen.1.hash(h);
        return;
    }
    #[cfg(test)]
    cost_tests::count_written();
    let json = feature_json(f, steps);
    json.hash(h);
    let entry = Arc::new((f.clone(), json));
    let mut written = WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
    if written.len() >= 1 << 15 {
        written.clear();
    }
    written.insert(f.id, entry);
}

fn feature_json(f: &Feature, steps: &ImportDigests) -> String {
    if matches!(&f.kind, FeatureKind::MeshImport { .. }) {
        let mut g = f.clone();
        let mut digests = Vec::new();
        if let FeatureKind::MeshImport { meshes, .. } = &mut g.kind {
            for (index, mesh) in meshes.iter_mut().enumerate() {
                digests.push(steps.mesh(f.id, index, mesh).unwrap_or_else(|| mesh_content_digest(mesh)));
                mesh.positions = Arc::from([]);
                mesh.triangles = Arc::from([]);
                mesh.corner_normals = Arc::from([]);
            }
        }
        // Keep the complete definition (including mesh order, names and colors) without
        // writing the large numeric arrays out again. Equal content in fresh Arcs hashes alike.
        return serde_json::to_string(&(g, digests)).unwrap_or_default();
    }
    if let FeatureKind::Import { step, records, .. } = &f.kind {
        let mut g = f.clone();
        if let FeatureKind::Import { step: text, records: kept, .. } = &mut g.kind {
            let digest = steps.get(f.id, step).unwrap_or_else(|| step_text_digest(step));
            // Records stand in by their digest, as the STEP text does.
            let of_records = records.as_ref().map(|r| steps.records_digest(f.id, r).unwrap_or_else(|| records_digest(r)));
            *kept = None;
            *text = match of_records {
                Some(r) => Arc::from(format!("{digest:016x}:{}:records:{r:016x}", step.len())),
                None => Arc::from(format!("{digest:016x}:{}", step.len())),
            };
        }
        return serde_json::to_string(&g).unwrap_or_default();
    }
    if let FeatureKind::Sketch { .. } = &f.kind {
        let mut g = f.clone();
        if let FeatureKind::Sketch { sketch, .. } = &mut g.kind {
            fingerprint_sketch(sketch);
        }
        return serde_json::to_string(&g).unwrap_or_default();
    }
    serde_json::to_string(f).unwrap_or_default()
}

/// Store in `stored` the solution `solved` holds for it: the positions, the circle and ellipse
/// radii, the arcs' end order and orientation (a signed sweep can swap them) and the
/// constraints' values. Nothing else: what evaluation derives for its own copy (an offset's
/// upgraded topology, refreshed links) stays out of the stored sketch. A `solved` that does not
/// match `stored` entity for entity is not adopted.
fn adopt_solution(stored: &mut solidcraft_sketch::Sketch, solved: &solidcraft_sketch::Sketch) -> bool {
    adopt_wires(stored, solved);
    if stored.patterns != solved.patterns {
        // Rebuild authored copies without storing topology derived from links or offsets.
        let mut updated = stored.clone();
        for pattern in &solved.patterns {
            let Some(index) = updated.patterns.iter().position(|p| p.id == pattern.id) else { return false };
            if updated.update_pattern(index, pattern.kind.clone()).is_err() {
                return false;
            }
        }
        if updated.patterns != solved.patterns || !adopt_solution(&mut updated, solved) {
            return false;
        }
        *stored = updated;
        return true;
    }
    // A text that follows parameters has other words, place or outlines than were stored.
    if !stored.adopt_texts(solved) {
        return false;
    }
    if stored.points.len() != solved.points.len()
        || stored.curves.len() != solved.curves.len()
        || stored.constraints.len() != solved.constraints.len()
        || stored.constraints.iter().zip(&solved.constraints).any(|(a, b)| a.id != b.id)
    {
        return false;
    }
    for (p, q) in stored.points.iter_mut().zip(&solved.points) {
        p.pos = q.pos;
    }
    for (c, d) in stored.curves.iter_mut().zip(&solved.curves) {
        match (&mut c.kind, &d.kind) {
            (CurveKind::Circle { r, .. }, CurveKind::Circle { r: s, .. }) | (CurveKind::Ellipse { r, .. }, CurveKind::Ellipse { r: s, .. }) => {
                *r = *s
            }
            (CurveKind::Arc { a, b, .. }, CurveKind::Arc { a: sa, b: sb, .. }) => {
                (*a, *b) = (*sa, *sb);
                c.reversed = d.reversed;
            }
            _ => {}
        }
    }
    for (c, d) in stored.constraints.iter_mut().zip(&solved.constraints) {
        if let Some(v) = d.kind.value() {
            c.kind.set_value(v);
        }
        solidcraft_sketch::keep_point_order(&mut c.kind, &d.kind);
    }
    true
}

/// [`adopt_solution`] for the drawn 3D curves, apart from the planar sketch (whose entities may
/// not pair up): where their constraints solved them for the new values, and those constraints
/// (their values, the places fixed points' expressions give) (#10608).
fn adopt_wires(stored: &mut solidcraft_sketch::Sketch, solved: &solidcraft_sketch::Sketch) {
    for w in stored.wires.iter_mut().filter(|w| w.link.is_none()) {
        if let Some(s) = solved.wires.iter().find(|s| s.id == w.id && s.link.is_none()) {
            w.fit.clone_from(&s.fit);
            w.pts.clone_from(&s.pts);
            w.arc = s.arc;
        }
    }
    if stored.wire_constraints.len() == solved.wire_constraints.len()
        && stored.wire_constraints.iter().zip(&solved.wire_constraints).all(|(a, b)| a.id == b.id)
    {
        stored.wire_constraints.clone_from(&solved.wire_constraints);
    }
}

impl Document {
    /// The values features are evaluated with: the parameters, and the design's length unit.
    pub fn evaluation_values(&self) -> BTreeMap<String, Value> {
        evaluation_values(self).0
    }

    /// A placement point read as the features read it: the parameters, and a bare number
    /// beside them in the design's length unit (#9015).
    pub fn eval_point(&self, p: &crate::PointExpr) -> Result<Vec3> {
        point(&self.evaluation_values(), p)
    }
}

/// The values features are evaluated with: the parameters, and the design's length unit.
fn evaluation_values(doc: &Document) -> (BTreeMap<String, Value>, BTreeMap<String, String>) {
    let (mut vals, perr) = doc.param_values();
    if let Some((Kind::Length, scale)) = expr::unit_info(doc.units.trim())
        && scale != 1.0
    {
        vals.insert(DESIGN_LEN.into(), Value { v: scale, len: 0, ang: 0 });
    }
    (vals, perr)
}

/// A feature's fingerprint in the chain after `prev`: its own inputs, where it is placed, and
/// the sheet metal or plastic rules it takes defaults from.
#[allow(clippy::too_many_arguments)]
fn chain_fingerprint(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    prev: u64,
    rolled_back: bool,
    steps: &ImportDigests,
    st: Option<&ModelState>,
) -> u64 {
    // A pose-only feature's placement is checked on its own ([`Model::poses`]), so moving the
    // parts it reads doesn't change the fingerprint of every feature after it (#12139).
    let placed = if pose_only(f) { 0 } else { placement_digest(doc, f, st) };
    let mut fp = fingerprint(doc, prev, f, vals, rolled_back, steps) ^ placed ^ construct_digest(doc, vals, f, st);
    // Sheet metal features also depend on the rules (and the parameters they use).
    if f.kind.is_sheet() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        fp.hash(&mut h);
        serde_json::to_string(&doc.sheet).unwrap_or_default().hash(&mut h);
        for r in &doc.sheet.rules {
            for (_, e, _) in r.fields() {
                for n in expr::references(e) {
                    vals.get(&n).map(|v| v.v.to_bits()).hash(&mut h);
                }
            }
        }
        fp = h.finish();
    }
    // Plastic features take defaults from the plastic rules.
    if f.kind.is_plastic() && !doc.plastic.is_empty() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        fp.hash(&mut h);
        serde_json::to_string(&doc.plastic).unwrap_or_default().hash(&mut h);
        for r in &doc.plastic.rules {
            for (_, e, _) in r.fields() {
                for n in expr::references(e) {
                    vals.get(&n).map(|v| v.v.to_bits()).hash(&mut h);
                }
            }
        }
        fp = h.finish();
    }
    // A frame member is built turned by the Rotate steps after it, which the chain up to it
    // doesn't cover.
    if let Some(turns) = turns_digest(doc, vals, f) {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        fp.hash(&mut h);
        turns.hash(&mut h);
        fp = h.finish();
    }
    // A pattern or mirror of features replays its sources' definitions wherever they are in the
    // timeline. One made with the timeline rolled back comes before them, where the chain doesn't
    // cover them: its copies kept the size the sources had when they were made.
    if let FeatureKind::Pattern { features, bodies, .. } | FeatureKind::Mirror { features, bodies, .. } = &f.kind
        && bodies.is_empty()
        && !features.is_empty()
    {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        fp.hash(&mut h);
        // When frame members become patternable or mirrorable, fold `turns_digest` into the replay.
        for g in replayed_sources(doc, features) {
            fingerprint(doc, 0, g, vals, false, steps).hash(&mut h);
        }
        fp = h.finish();
    }
    fp
}

/// The Rotate steps after frame member `f` that may turn it ([`frame_member::turn_steps`]), every
/// one, since one whose angle fails gives way to an earlier one, and the values of the parameters
/// their angles use. None for a frame member without them and any other feature, whose keys are
/// then as they were before Rotate steps.
fn turns_digest(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature) -> Option<u64> {
    let FeatureKind::FrameMember { .. } = &f.kind else { return None };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut any = false;
    for (line, e) in frame_member::turn_steps(doc, crate::FeatureId(f.id)) {
        any = true;
        line.hash(&mut h);
        e.hash(&mut h);
        for n in expr::references(e) {
            // Its unit too: `30` and `30 rad` are the same number.
            vals.get(&n).map(|v| (v.v.to_bits(), v.len, v.ang)).hash(&mut h);
        }
    }
    any.then(|| h.finish())
}

/// The features a pattern or mirror of `features` replays: each source, and the sources of one
/// that is itself a pattern or mirror of features (see `copy_plan`), each once.
fn replayed_sources<'a>(doc: &'a Document, features: &[crate::FeatureId]) -> Vec<&'a Feature> {
    let mut out: Vec<&Feature> = Vec::new();
    let mut todo: Vec<crate::FeatureId> = features.to_vec();
    // A bound on the walk, as on the copy plan's nesting: a design can't make it run on.
    for _ in 0..1024 {
        let Some(id) = todo.pop() else { break };
        let Ok(g) = source_feature(doc, id) else { continue };
        if out.iter().any(|x| x.id == g.id) {
            continue;
        }
        if let FeatureKind::Pattern { features, bodies, components, .. } = &g.kind
            && bodies.is_empty()
            && components.is_empty()
        {
            todo.extend(features.iter().copied());
        } else if let FeatureKind::Mirror { features, bodies, .. } = &g.kind
            && bodies.is_empty()
        {
            todo.extend(features.iter().copied());
        }
        out.push(g);
    }
    out
}

/// Every string in `v` (names of planes, axes, bodies and features among them).
fn json_strings<'a>(v: &'a serde_json::Value, out: &mut std::collections::BTreeSet<&'a str>) {
    match v {
        serde_json::Value::String(s) => {
            out.insert(s);
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| json_strings(x, out)),
        serde_json::Value::Object(o) => o.values().for_each(|x| json_strings(x, out)),
        _ => {}
    }
}

/// The construction planes, axes and points feature `f` names (#6183), and those they are built
/// on in turn, as `st` holds them (or by their definitions, for one not evaluated by then). A
/// pattern or mirror also reads those its replayed features name. Taking any string in the
/// definition that is a construction's name may take one too many, never one too few: the
/// other construction geometry no longer goes into every feature's key.
fn construct_digest(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: Option<&ModelState>) -> u64 {
    if !doc.features.iter().any(|f| f.kind.is_construction()) {
        return 0;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut todo: Vec<&Feature> = vec![f];
    if let FeatureKind::Pattern { features, .. } | FeatureKind::Mirror { features, .. } = &f.kind {
        todo.extend(features.iter().filter_map(|id| source_feature(doc, *id).ok()));
    }
    let mut seen = std::collections::BTreeSet::new();
    // Nothing named: 0, as in a design without construction geometry (adding the first
    // construction leaves the keys of the features that do not name it alone).
    let mut named = false;
    // Bounded: a chain of constructions deeper than this is hashed as far as it goes, and the
    // whole definition list is the fallback.
    let mut steps = 0usize;
    while let Some(g) = todo.pop() {
        steps += 1;
        if steps > 4096 {
            named = true;
            "too deep".hash(&mut h);
            for c in doc.features.iter().filter(|c| c.kind.is_construction()) {
                fingerprint(doc, 0, c, vals, false, &ImportDigests::default()).hash(&mut h);
            }
            break;
        }
        let Ok(json) = serde_json::to_value(&g.kind) else {
            named = true;
            "unreadable".hash(&mut h);
            continue;
        };
        let mut names = std::collections::BTreeSet::new();
        json_strings(&json, &mut names);
        // Constructions are named by id too (a sketch's plane, #6858).
        let mut ids = std::collections::BTreeSet::new();
        crate::walk::Walk::walk_refs(&g.kind, &mut |r| {
            if let crate::feature_refs::Ref::Feature(id) = r {
                ids.insert(id.0);
            }
        });
        for c in doc.features.iter().filter(|c| c.kind.is_construction() && c.id != g.id && (ids.contains(&c.id) || names.contains(c.name.as_str())))
        {
            if !seen.insert(c.id) {
                continue;
            }
            named = true;
            c.name.hash(&mut h);
            match st.map(|st| st.construct.iter().filter(|x| x.name == c.name).collect::<Vec<_>>()).filter(|v| !v.is_empty()) {
                // Evaluated already: as it came out (it may stand on faces and edges).
                Some(done) => serde_json::to_string(&done).unwrap_or_default().hash(&mut h),
                None => {
                    fingerprint(doc, 0, c, vals, false, &ImportDigests::default()).hash(&mut h);
                    todo.push(c);
                }
            }
        }
    }
    if named { h.finish() } else { 0 }
}

/// Document-level inputs any feature may read: sheet metal and plastic rules (and the
/// parameters they use), components and body placement. Construction geometry is not among
/// them: each feature reads only the constructions it names ([`construct_digest`]).
fn context_digest(doc: &Document, vals: &BTreeMap<String, Value>) -> u64 {
    digest_context(doc, vals, true)
}

/// [`context_digest`] without the design's components: units and the sheet metal and plastic
/// rules. A change told by what a feature read ([`Relative`]) takes the feature's own components
/// instead ([`EvalKey`]), and is placed in the components as they are when it is reused.
fn shared_context_digest(doc: &Document, vals: &BTreeMap<String, Value>) -> u64 {
    digest_context(doc, vals, false)
}

fn digest_context(doc: &Document, vals: &BTreeMap<String, Value>, components: bool) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    doc.units.hash(&mut h);
    serde_json::to_string(&doc.sheet).unwrap_or_default().hash(&mut h);
    serde_json::to_string(&doc.plastic).unwrap_or_default().hash(&mut h);
    if components {
        serde_json::to_string(&doc.components).unwrap_or_default().hash(&mut h);
        serde_json::to_string(&doc.body_components).unwrap_or_default().hash(&mut h);
    }
    for e in doc.sheet.rules.iter().flat_map(|r| r.fields().map(|(_, e, _)| e)) {
        for n in expr::references(e) {
            vals.get(&n).map(|v| v.v.to_bits()).hash(&mut h);
        }
    }
    for r in &doc.plastic.rules {
        for (_, e, _) in r.fields() {
            for n in expr::references(e) {
                vals.get(&n).map(|v| v.v.to_bits()).hash(&mut h);
            }
        }
    }
    h.finish()
}

/// What the validation mode adds to the context digest: nothing for checking every step (how
/// every evaluation before the mode checked, so its results and saved caches stand), so a result
/// is taken from the cache only under the mode it was evaluated in.
fn validation_digest(validation: kernel::Validation) -> u64 {
    match validation {
        kernel::Validation::EachStep => 0,
        v => {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            ("validation", v.id()).hash(&mut h);
            h.finish()
        }
    }
}

/// What the tolerance heal preference ([`kernel::heals_tolerance_defects`]) adds to the context
/// digest: nothing with it on (the default), so a result is taken from the cache only as healed
/// as it was evaluated (an import's parts, and Repair's heal first).
pub(crate) fn heal_digest(heal: bool) -> u64 {
    if heal {
        return 0;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ("tolerance heal", "off").hash(&mut h);
    h.finish()
}

/// SMLib's check of every body a feature made or changed (in `after`, not in `before`, the
/// state it started from), once its steps left their checks to its end: the first that fails is
/// the feature's error, naming the body.
fn check_kept(before: &ModelState, after: &ModelState) -> kernel::Result<()> {
    for b in &after.bodies {
        if before.bodies.iter().any(|old| old.body.same_shape(&b.body)) {
            continue;
        }
        // A thread output already checked (only successful checks are recorded) passed.
        if checked_thread_output(&b.body) {
            #[cfg(test)]
            cost_tests::count(|c| c.reused_checks += 1);
            continue;
        }
        check_feature_body(&b.body).map_err(|e| kept_refused(&b.name, &e, &replaced(before, after)))?;
    }
    Ok(())
}

/// [`check_kept`] of bodies whose checks were detached from them ([`kernel::detach_kept`]), by
/// body name, in order: the first that fails is the feature's error, as there. `inputs` are the
/// bodies the feature replaced ([`replaced`]), for that error.
fn run_kept(checks: Vec<(String, kernel::KeptCheck)>, inputs: Vec<(String, Body, bool)>) -> kernel::Result<()> {
    for (name, check) in checks {
        check.run().map_err(|e| kept_refused(&name, &e, &inputs))?;
    }
    Ok(())
}

/// The bodies of `before` a feature that made `after` replaced or used up: its inputs, whose own
/// check says whether a refusal of its result is the feature's doing. Each with whether the
/// feature used it up (no body of `after` has its name, as a join's tools), so that a refusal of
/// one result is blamed on its own former self or on bodies it consumed, never on another body
/// the same feature changed beside it.
fn replaced(before: &ModelState, after: &ModelState) -> Vec<(String, Body, bool)> {
    before
        .bodies
        .iter()
        .filter(|old| !after.bodies.iter().any(|b| b.body.same_shape(&old.body)))
        .map(|old| (old.name.clone(), old.body.clone(), !after.bodies.iter().any(|b| b.name == old.name)))
        .collect()
}

/// The feature's error when the check of body `name`, which it keeps, refused it (`e`): naming
/// the bodies among `inputs` that went into it (its own former self, by name, and the bodies the
/// feature used up) whose SMLib form, as the feature took them, fails SMLib's check
/// ([`kernel::failing_check`]), since the result's check reports their defects too. Checked only
/// here, once the result is refused.
///
/// That is a check of each input as it is now, not a replay of the check that kept it at the end
/// of its own feature, which ran on that step's own result ([`kernel::check_kept`]; for a
/// boolean, as the separate solids a non-manifold result exports to). The body decoded from a
/// result is a different SMLib form, which can fail where the result passed, so the message says
/// what was checked: the body as it was before this feature, not that its own feature let a
/// failure through.
fn kept_refused(name: &str, e: &kernel::KernelError, inputs: &[(String, Body, bool)]) -> kernel::KernelError {
    let failing: Vec<&str> = inputs
        .iter()
        .filter(|(n, _, used_up)| *used_up || n == name)
        .filter(|(_, b, _)| kernel::failing_check(b).is_ok_and(|c| c.is_some()))
        .map(|(n, _, _)| n.as_str())
        .collect();
    let before = match failing.as_slice() {
        [] => String::new(),
        [only] if *only == name => " (SMLib's check fails it as it was before this feature)".to_string(),
        [only] => format!(" (SMLib's check fails body `{only}` as it was before this feature)"),
        many => format!(
            " (SMLib's check fails bodies {} as they were before this feature)",
            many.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ")
        ),
    };
    match e {
        // A panic the check caught stays one, so it is not cached as an ordinary refusal.
        kernel::KernelError::Internal(m) => kernel::KernelError::Internal(format!("body `{name}`{before}: {m}")),
        _ => kernel::KernelError::Failed(format!("body `{name}`{before}: {e}")),
    }
}

/// Whether `after`, a state feature `f` evaluated to from `before` in an earlier evaluation, is
/// still its result in `doc`: a feature saved before format 14 names the pieces it splits a body
/// into as the later features pick them ([`crate::lineage::order_legacy_lumps`]), which neither
/// its fingerprint nor its cache key cover. (A script replays those features after it: the state
/// made before they were there named the pieces otherwise.)
fn legacy_current(doc: &Document, f: &Feature, before: &ModelState, after: &ModelState) -> bool {
    !f.shell_piece_order || after.legacy_picks.get(&f.id).copied().unwrap_or(0) == crate::lineage::legacy_picks_digest(&after.bodies, before, doc, f)
}

/// The cache key of a feature evaluation: its own inputs, the document context and the state it
/// starts from. Features that replay other features' definitions (patterns and mirrors of
/// features) also take the chain fingerprint, which covers every earlier definition.
fn cache_key(f: &Feature, chain_fp: u64, local: u64, ctx: u64, state: &ModelState) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    local.hash(&mut h);
    ctx.hash(&mut h);
    state.digest().hash(&mut h);
    f.id.hash(&mut h);
    let replays = match &f.kind {
        FeatureKind::Pattern { features, .. } | FeatureKind::Mirror { features, .. } => !features.is_empty(),
        _ => false,
    };
    if replays {
        chain_fp.hash(&mut h);
    }
    h.finish()
}

/// What a feature reads of placements: components (their first placement), occurrence paths
/// and bodies in its timeline input, whose geometry it finds in its evaluating frame, or all.
#[derive(Default)]
struct PlacementReads {
    all: bool,
    /// A body named that `st` lacks: a further placement's (`Block (Part:2)`), found through
    /// its occurrence (#9957), or gone.
    unlisted: bool,
    components: std::collections::BTreeSet<u64>,
    paths: Vec<Vec<u64>>,
    bodies: std::collections::BTreeSet<String>,
}

impl PlacementReads {
    fn is_empty(&self) -> bool {
        !self.all && self.components.is_empty() && self.paths.is_empty() && self.bodies.is_empty()
    }

    /// A body's complete placement in the feature's input state, including its offset. Later
    /// transfers do not change where an earlier feature read it.
    fn placed_body(&mut self, st: Option<&ModelState>, name: &str) {
        if st.is_some_and(|st| st.body(name).is_some()) {
            self.bodies.insert(name.to_owned());
        } else {
            // A further placement's body (`Block (Part:2)`) moves with its occurrence (#20040).
            self.all = true;
            self.unlisted = true;
        }
    }

    /// The bodies To / From Object's stored face names find, each where it is placed: evaluation
    /// finds a named face by its name ([`with_named_face_points`] fails a feature whose name finds
    /// none). Everything when a name is missing or empty, finds no one body, or `st` is `None`:
    /// the face is then found by its point, on whatever body lies there.
    fn object_faces(&mut self, st: Option<&ModelState>, names: Option<&[String]>) {
        let bodies: Option<Vec<&str>> = st.zip(names).and_then(|(st, names)| {
            let first_of_its_name = |b: &&ModelBody| st.body(&b.name).is_some_and(|first| std::ptr::eq(first, *b));
            names.iter().map(|n| crate::name_lookup::face_body(st, n).filter(first_of_its_name).map(|b| b.name.as_str())).collect()
        });
        match bodies {
            Some(bodies) => bodies.into_iter().for_each(|b| self.placed_body(st, b)),
            None => self.all = true,
        }
    }

    /// The same target selection as `apply_traced`, expanded only over the timeline input.
    fn drawn_targets(&mut self, st: Option<&ModelState>, f: &Feature) {
        match st {
            Some(st) => self.targets(st, f, &feature_op(f).1),
            None => self.all = true,
        }
    }

    /// The bodies `apply_traced` changes for `f` with `targets`.
    fn targets(&mut self, st: &ModelState, f: &Feature, targets: &[String]) {
        let names: &[String] = if targets.is_empty() { &f.participants } else { targets };
        let all = names.len() == 1 && names.first().is_some_and(|n| n == "*");
        if all || names.is_empty() {
            self.bodies.extend(st.bodies.iter().filter(|b| if all { !b.envelope } else { b.in_default_scope(f) }).map(|b| b.name.clone()));
        } else {
            for name in names {
                self.placed_body(Some(st), name);
            }
        }
    }

    /// The component of body `name` in `st`; every component and every placement when the body
    /// is not there (a further placement's body, or the body nearest its point, wherever that is).
    fn body(&mut self, doc: &Document, st: Option<&ModelState>, name: &str) {
        match st.and_then(|st| st.body(name)) {
            Some(b) => {
                self.components.insert(doc.model_body_component(b));
            }
            None => {
                self.all = true;
                self.unlisted = true;
            }
        }
    }

    /// The component of sketch `id`, and the root when it holds wires fixed in the world (they
    /// are read through the reader's whole placement).
    fn sketch(&mut self, doc: &Document, id: u64) {
        self.components.insert(doc.feature(id).map(|f| f.component).unwrap_or(0));
        if doc.sketch(id).is_ok_and(|sk| sk.wires.iter().any(|w| w.space == solidcraft_sketch::WireSpace::World)) {
            self.components.insert(0);
        }
    }

    fn link(&mut self, doc: &Document, st: Option<&ModelState>, source: &solidcraft_sketch::LinkSource) {
        use solidcraft_sketch::LinkSource as L;
        match source {
            L::Edge { body, .. } | L::Face { body, .. } | L::Body { body } | L::Vertex { body, .. } | L::Iso { body, .. } | L::Spun { body, .. } => {
                self.body(doc, st, body)
            }
            L::SketchCurve { sketch, .. } | L::SketchPoint { sketch, .. } => self.sketch(doc, *sketch),
            L::OnSurface { sketch, body, .. } => {
                self.sketch(doc, *sketch);
                self.body(doc, st, body);
            }
            L::Intersection { a, b } => {
                self.link(doc, st, a);
                self.link(doc, st, b);
            }
            // In the sketch's own frame: the component's origin, axes and planes, its own axes, text.
            // A construction point is kept in its own component's frame: the sketch reads where
            // that component is placed (#12075).
            L::ConstructionPoint { name } => match doc.find_feature(name) {
                Some(point) => {
                    self.components.insert(point.component);
                }
                None => self.all = true,
            },
            // A view edge is refused in a model sketch, whatever the model holds.
            L::Origin | L::Axis { .. } | L::SketchAxis { .. } | L::Plane { .. } | L::Text { .. } | L::ViewEdge { .. } => {}
        }
    }
}

/// The cache keys of a feature's evaluation.
struct EvalKey {
    /// From the whole model before it ([`cache_key`]).
    whole: u64,
    /// From what it reads alone, with that ([`Feature::reads`]); `None` when it may read
    /// anything.
    reads: Option<(u64, Reads)>,
    /// From what it reads alone, without the names of the bodies it does not read: the key of a
    /// change that holds whatever those bodies are ([`Relative`]). Set with `reads`.
    relative: Option<u64>,
}

impl EvalKey {
    #[allow(clippy::too_many_arguments)]
    fn new(
        doc: &Document,
        vals: &BTreeMap<String, Value>,
        f: &Feature,
        fp: u64,
        ctx: u64,
        shared: u64,
        st: &ModelState,
        steps: &ImportDigests,
        geo: u64,
    ) -> EvalKey {
        // A frame member is built turned by the Rotate steps after it.
        let local = fingerprint(doc, 0, f, vals, false, steps)
            ^ placement_digest(doc, f, Some(st))
            ^ construct_digest(doc, vals, f, Some(st))
            ^ turns_digest(doc, vals, f).unwrap_or(0)
            ^ depth_digest(st);
        let whole = cache_key(f, fp, local, ctx, st);
        let reads = f.reads(doc, st).map(|read| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            "reads".hash(&mut h);
            own_inputs(doc, vals, f, local, ctx, geo, steps, &mut h);
            // Every body's name and component: new names avoid the names, a cut's pieces are
            // numbered past them, a tool's default targets are its component's bodies.
            for b in &st.bodies {
                b.name.hash(&mut h);
                b.component.hash(&mut h);
                b.transferred.hash(&mut h);
            }
            for &i in &read.bodies {
                i.hash(&mut h);
                st.bodies.get(i).map(|b| (b.key, b.feature)).hash(&mut h);
            }
            for id in &read.sketches {
                id.hash(&mut h);
                st.sketch_keys.get(id).hash(&mut h);
            }
            st.body_counter.hash(&mut h);
            // A body numbered from the counter skips the numbers reserved past it.
            if f.body_names.is_empty() || f.body_names.iter().any(|n| n.is_empty() || st.body(n).is_some()) {
                for n in st.reserved_body_names.iter().filter(|n| crate::is_counted_body_name(n)) {
                    n.hash(&mut h);
                }
            }
            // A cut drops the threads of bodies it removes.
            serde_json::to_string(&st.threads).unwrap_or_default().hash(&mut h);
            (h.finish(), read)
        });
        let relative = reads.as_ref().map(|(_, read)| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            "relative".hash(&mut h);
            own_inputs(doc, vals, f, local, shared, geo, steps, &mut h);
            // Its own component and those it lies in, not every component of the design: a
            // component deleted or added elsewhere changes nothing it reads.
            let mut c = f.component;
            for _ in 0..=crate::MAX_PLACEMENT_DEPTH {
                let comp = doc.components.iter().find(|x| x.id == c);
                serde_json::to_string(&comp).unwrap_or_default().hash(&mut h);
                match comp {
                    Some(comp) if comp.id != 0 && comp.parent != comp.id => c = comp.parent,
                    _ => break,
                }
            }
            // The bodies it reads by name, not by where they stand: bodies before them come and go.
            let mut names = Vec::with_capacity(read.bodies.len());
            for &i in &read.bodies {
                let b = st.bodies.get(i);
                b.map(|b| (&b.name, b.key, b.feature, b.component, b.transferred)).hash(&mut h);
                names.extend(b.map(|b| b.name.as_str()));
            }
            for id in &read.sketches {
                id.hash(&mut h);
                st.sketch_keys.get(id).hash(&mut h);
            }
            // A body takes the name the feature stores for it only while no other body has it.
            for n in &f.body_names {
                (n, st.body(n).is_some()).hash(&mut h);
            }
            // A cut drops the threads of bodies it removes.
            for t in st.threads.iter().filter(|t| names.contains(&t.body.as_str())) {
                serde_json::to_string(t).unwrap_or_default().hash(&mut h);
            }
            h.finish()
        });
        EvalKey { whole, reads, relative }
    }
}

/// What the naming depth adds to a feature's keys: nothing up to the limit, past which the bodies
/// it makes name their faces by position. Every body before it counts, so features it reads
/// nothing of put it past the limit: a result named one way is not kept where the design
/// evaluated afresh names it the other, and the references to it find other faces or none (#20956).
fn depth_digest(st: &ModelState) -> u64 {
    if !crate::naming::past_limit(st) {
        return 0;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    "past the naming limit".hash(&mut h);
    h.finish()
}

/// A feature's own inputs, for the keys of what it reads ([`EvalKey`]): its definition and
/// placement (`local`), the document context, its id, the definitions a pattern replays and a
/// sketch's stored solve.
#[allow(clippy::too_many_arguments)]
fn own_inputs(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    local: u64,
    ctx: u64,
    geo: u64,
    steps: &ImportDigests,
    h: &mut impl Hasher,
) {
    local.hash(h);
    ctx.hash(h);
    f.id.hash(h);
    // An import reads the file it names and nothing of the model, so it keeps its result
    // while the features before it are the same: a part that moves leaves the file
    // unread (#17310). The features' own inputs, without where they are placed, are in
    // the key: a change to them (a new length) reads the file again.
    if matches!(f.kind, FeatureKind::Import { .. }) {
        geo.hash(h);
    }
    // A pattern replays its features' definitions.
    if let FeatureKind::Pattern { features, .. } = &f.kind {
        for id in features {
            source_feature(doc, *id).ok().map(|src| fingerprint(doc, 0, src, vals, false, steps)).hash(h);
        }
    }
    // A sketch's stored solve exactly: its fingerprint rounds it to a grid, and a sketch
    // that rounds alike gives back the solve it was taken from, which an edit compares
    // exactly (deleting a dimension that changed its points by less than the grid).
    if let FeatureKind::Sketch { sketch, .. } = &f.kind {
        for p in &sketch.points {
            (p.pos.x.to_bits(), p.pos.y.to_bits()).hash(h);
        }
    }
}

/// What a feature reads of the model besides every body's name and component: bodies by index,
/// sketches by feature id.
#[derive(Clone, Debug, Default)]
struct Reads {
    bodies: Vec<usize>,
    sketches: Vec<u64>,
}

/// A primitive or a coil of its own section: tools from numbers alone. A coil sweeping a
/// sketch's profile reads that sketch.
fn numeric_tool(kind: &FeatureKind) -> bool {
    matches!(
        kind,
        FeatureKind::Box { .. }
            | FeatureKind::Cylinder { .. }
            | FeatureKind::Sphere { .. }
            | FeatureKind::Torus { .. }
            | FeatureKind::Coil { sketch: None, .. }
    )
}

/// What a sketch's projection of `source` reads: the bodies and sketches it names. `None` when
/// a body it names is not there (it then takes the body nearest its point, wherever that is).
fn link_reads(st: &ModelState, source: &solidcraft_sketch::LinkSource, read: &mut Reads) -> Option<()> {
    use solidcraft_sketch::LinkSource as L;
    let body = |name: &str, read: &mut Reads| -> Option<()> {
        read.bodies.push(st.bodies.iter().position(|b| b.name == name)?);
        Some(())
    };
    match source {
        L::Edge { body: b, .. }
        | L::Face { body: b, .. }
        | L::Body { body: b }
        | L::Vertex { body: b, .. }
        | L::Iso { body: b, .. }
        | L::Spun { body: b, .. } => body(b, read),
        L::SketchCurve { sketch, .. } | L::SketchPoint { sketch, .. } => {
            read.sketches.push(*sketch);
            Some(())
        }
        L::OnSurface { sketch, body: b, .. } => {
            read.sketches.push(*sketch);
            body(b, read)
        }
        L::Intersection { a, b } => {
            link_reads(st, a, read)?;
            link_reads(st, b, read)
        }
        // Kept in its own component's frame, which these reads do not follow: key on the whole model.
        L::ConstructionPoint { .. } => None,
        // In the sketch's own frame (construction axes it names are in its key), or text. A view
        // edge is refused in a model sketch, whatever the model holds.
        L::Origin | L::Axis { .. } | L::SketchAxis { .. } | L::Plane { .. } | L::Text { .. } | L::ViewEdge { .. } => Some(()),
    }
}

/// How far from a fillet's or chamfer's first edge point (mm) the edge it finds may lie for its
/// body to be known from the bodies around the point alone.
const BLEND_REACH: f64 = 1.0;

/// The bodies a fillet or chamfer with no body named may pick ([`blend_target`]): those whose
/// box reaches within [`BLEND_REACH`] of its first edge point. `None` when none of them has an
/// edge that near, as a body further away could then be the nearest; or around a mesh body.
fn blend_candidates(st: &ModelState, edges: &[Vec3]) -> Option<Vec<usize>> {
    if edges.is_empty() {
        return None;
    }
    // Each pick finds its body among all of them: the read set is every body near any pick, and
    // a pick with no edge near it may take one anywhere.
    let mut all = Vec::new();
    for &p in edges {
        let near: Vec<usize> =
            st.bodies.iter().enumerate().filter(|(_, b)| b.outline_bounds().inflated(BLEND_REACH).contains(p)).map(|(i, _)| i).collect();
        let mut hit = false;
        for b in near.iter().filter_map(|i| st.bodies.get(*i)) {
            if b.body.is_mesh() {
                return None;
            }
            let tol = (b.body.size() * 2e-3).max(1e-3);
            // A note names every body with an edge within twice this distance
            // ([`body_on_edge_noted`]): those too are among the bodies near.
            hit |= matches!(b.body.nearest_edge(p, tol), Ok(Some((_, d))) if 2.0 * d + 1e-3 <= BLEND_REACH);
        }
        if !hit {
            return None;
        }
        all.extend(near);
    }
    all.sort_unstable();
    all.dedup();
    Some(all)
}

/// A pattern whose copies are placed by numbers alone: not along a sketch path, nor about a
/// model edge or face, or a feature named for its axis.
fn fixed_pattern(pattern: &crate::PatternKind) -> bool {
    match pattern {
        crate::PatternKind::Rectangular { .. } => true,
        crate::PatternKind::Circular { axis_ref, axis_feature, .. } => axis_ref.is_none() && axis_feature.is_none(),
        crate::PatternKind::Path { .. } => false,
    }
}

/// A plane given by numbers alone or a construction plane (its geometry is in the feature's key,
/// [`construct_digest`]): no face or edge to find.
fn fixed_plane(plane: &PlaneRef) -> bool {
    match plane {
        PlaneRef::Origin { .. } | PlaneRef::Custom { .. } | PlaneRef::Placed { .. } | PlaneRef::Construction { .. } => true,
        // Turned about an edge or face of the model (or through a point of it): that geometry
        // is read too, as `plane_reads_geometry` says.
        PlaneRef::AtAngle { line: Some(_), .. } | PlaneRef::AtAngle { pivot: Some(_), .. } => false,
        PlaneRef::Offset { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => fixed_plane(base),
        _ => false,
    }
}

impl Feature {
    /// What this feature reads of `st`, when that is known to be all it reads: a primitive or a
    /// coil (or a rectangular or circular pattern of them) on the bodies it names or its
    /// component's bodies, a move or pattern of named bodies, a sketch on a plane given by
    /// numbers or a construction plane (its projections reading the bodies and sketches they
    /// name), an extrude or revolve from a sketch of its component on the bodies it names or its
    /// component's bodies (no To / From Object, through all or taper), a fillet or chamfer of
    /// the body its edges lie on (#6183), an import (the file it names, and nothing of the
    /// model), a boundary surface through curves of a sketch of its component. Construction geometry it names is in its key already
    /// ([`construct_digest`]). Never with a face picked by name, or an edge picked by name outside
    /// a fillet or chamfer; everything else may read anything (faces, To Object, `*` for every
    /// body).
    fn reads(&self, doc: &Document, st: &ModelState) -> Option<Reads> {
        let by_name = |f: &Feature| f.face_names.iter().all(String::is_empty) && f.edge_names.iter().all(String::is_empty) && f.edge_refs.is_empty();
        let blend = matches!(self.kind, FeatureKind::Fillet { .. } | FeatureKind::Chamfer { .. });
        // A fillet or chamfer finds its edges by name on its own body alone.
        let faces_by_name = self.face_names.iter().any(|n| !n.is_empty());
        // A sheet's record (its surfaces, bends, cuts and pockets) is not carried by a replayed
        // change, only its body: with sheets in the design, every feature is evaluated again
        // (#6748, as the flat model's sheets were).
        if !st.surface_sheets.is_empty() || faces_by_name || (!blend && !by_name(self)) {
            return None;
        }
        let named = |names: &[String]| -> Option<Vec<usize>> {
            if names.iter().any(|n| n == "*") {
                return None;
            }
            // A name evaluation may refuse as ambiguous, for other bodies of this component
            // ([`named_target`]): what it finds is not all that is read.
            names.iter().map(|n| named_target(doc, st, self, n).ok()).collect()
        };
        // The bodies a tool operates on, as `apply_traced` picks them.
        let operated = |(op, targets): (Operation, Vec<String>)| -> Option<Vec<usize>> {
            let targets = if targets.is_empty() { &self.participants } else { &targets };
            match op {
                // A new body reads none of the bodies it names, but fails without them.
                Operation::NewBody if targets.is_empty() || targets.iter().all(|n| n == "*") => Some(Vec::new()),
                Operation::NewBody => named(targets).map(|_| Vec::new()),
                _ if targets.is_empty() => Some(
                    st.bodies.iter().enumerate().filter(|(_, b)| b.component == self.component || b.feature == self.id).map(|(i, _)| i).collect(),
                ),
                _ => named(targets),
            }
        };
        let bodies = |bodies| Some(Reads { bodies, sketches: Vec::new() });
        let sketch = |s: &crate::SketchId| {
            let own = doc.feature(s.0).is_some_and(|x| x.component == self.component);
            own.then(|| Reads { bodies: Vec::new(), sketches: vec![s.0] })
        };
        match &self.kind {
            // A cylinder placed by reference edges reads the faces they lie on.
            k if numeric_tool(k) && hole_references(self).is_empty() => bodies(operated(feature_op(self))?),
            // An import reads the file it names (in its own fingerprint) and nothing of the
            // model: its bodies are its own, and their names follow the names the model
            // already has (in the key of what it reads). A part that moves elsewhere in the
            // timeline leaves it as it was, and the file is not read again (#17310).
            FeatureKind::Import { .. } => Some(Reads::default()),
            FeatureKind::Pattern { bodies: copied, pattern, .. } if !copied.is_empty() && fixed_pattern(pattern) => {
                bodies(named(&crate::plain::<String, _>(copied))?)
            }
            FeatureKind::Pattern { features, pattern, .. } if !features.is_empty() && fixed_pattern(pattern) => {
                let mut read = Vec::new();
                for id in features {
                    let src =
                        source_feature(doc, *id).ok().filter(|src| numeric_tool(&src.kind) && hole_references(src).is_empty() && by_name(src))?;
                    // As `replay` picks them: the source's targets, else its participants.
                    let (op, targets) = feature_op(src);
                    let targets = if targets.is_empty() { src.participants.clone() } else { targets };
                    read.extend(operated((op, targets))?);
                }
                read.sort_unstable();
                read.dedup();
                bodies(read)
            }
            // A turn about an edge or round face of the model reads that geometry too (#17974).
            FeatureKind::Move { rotate_about: Some(_), .. } => None,
            FeatureKind::Move { bodies: moved, .. } => bodies(named(&crate::plain::<String, _>(moved))?),
            FeatureKind::Combine { target, tools, .. } => {
                let mut names = vec![target.0.clone()];
                names.extend(crate::plain::<String, _>(tools));
                bodies(named(&names)?)
            }
            FeatureKind::Sketch { plane, sketch } if fixed_plane(plane) => {
                let mut read = Reads::default();
                for link in &sketch.links {
                    link_reads(st, &link.source, &mut read)?;
                }
                read.bodies.sort_unstable();
                read.bodies.dedup();
                read.sketches.sort_unstable();
                read.sketches.dedup();
                Some(read)
            }
            FeatureKind::Extrude { sketch: s, extent, .. }
                if extent.to.is_none() && extent.from.is_none() && !extent.through_all && extent.taper.is_none() =>
            {
                Some(Reads { bodies: operated(feature_op(self))?, ..sketch(s)? })
            }
            // About an edge or round face of the model: that geometry is read too.
            FeatureKind::Revolve { axis: crate::AxisRef::Model { .. }, .. } => None,
            FeatureKind::Revolve { sketch: s, to: None, .. } => Some(Reads { bodies: operated(feature_op(self))?, ..sketch(s)? }),
            FeatureKind::Fillet { edges, body, .. } | FeatureKind::Chamfer { edges, body, .. } => match body {
                Some(n) => bodies(named(std::slice::from_ref(&n.0))?),
                None => bodies(blend_candidates(st, &crate::plain(edges))?),
            },
            // A new body through curves of one sketch of its component: an edit of another
            // sketch leaves it as it was (#17714: a delete in a later sketch rebuilt a patch
            // whose edge tolerances take most of a second to prove, twice per check).
            FeatureKind::SurfaceBoundary { sketch: s, .. } => sketch(s),
            _ => None,
        }
    }
}

/// What a feature changed of the model (see [`ModelState::change_from`]): the bodies after it,
/// the sketch it solved and the body counter.
#[derive(Clone, Debug)]
struct Change {
    bodies: Arc<[Changed]>,
    sketch: Option<Arc<SolvedSketch>>,
    body_counter: usize,
    /// A fillet's or chamfer's records of the edges it blended and the radii it used.
    blend: Option<Vec<Vec3>>,
    fitted: Option<Vec<f64>>,
    tapered: Option<Vec<(Vec3, kernel::Taper)>>,
    /// A join's touching pieces it kept as bodies of their own.
    kept_separate: Option<KeptSeparate>,
    /// A body Pattern's sources it left out.
    pattern_left_out: Option<Vec<usize>>,
    /// Where the feature found its named faces.
    named_faces: Option<Vec<Vec3>>,
    /// The same change whatever the bodies the feature did not read, when it can be told so.
    relative: Option<Arc<Relative>>,
}

/// A change told by the bodies a feature read, by name, and the bodies it made after every
/// other: it holds on any model whose bodies it read are the same, whatever other bodies come
/// before, between or after them (see [`ModelState::with_relative`]).
#[derive(Debug)]
struct Relative {
    /// Each body it read, in order: what it became.
    read: Vec<(String, Fate)>,
    /// Bodies it made under names the feature stores, after all others.
    made: Vec<ModelBody>,
    /// The body counter a stored `BodyN` name among them counted up to (0: none).
    counter: usize,
}

#[derive(Debug)]
enum Fate {
    Kept,
    Changed(Box<ModelBody>),
    Removed,
}

#[derive(Clone, Debug)]
enum Changed {
    /// The body at this index before the feature, as it was.
    Kept(usize),
    /// A body the feature made or changed.
    Made(Box<ModelBody>),
}

impl Change {
    /// What the bodies it made cost.
    fn retained_bytes(&self) -> usize {
        let made: usize = self
            .bodies
            .iter()
            .map(|b| match b {
                Changed::Made(b) => b.body.retained_bytes(),
                Changed::Kept(_) => 0,
            })
            .sum();
        made + self.bodies.len() * 64 + self.sketch.as_ref().map_or(0, |s| s.retained_bytes())
    }
}

impl Relative {
    /// `after`'s bodies as a change of `before`'s by feature `f`, which read the bodies `read`:
    /// every body it did not read stands where it stood, each body it read is kept, changed under
    /// its own name or removed in place, and the bodies it made under the names it stores come
    /// after all of them (only when it changed none: pieces of a split stand by their body).
    /// `None` when the change is not of that form.
    fn of(before: &ModelState, after: &ModelState, f: &Feature, read: &Reads) -> Option<Relative> {
        let mut is_read = vec![false; before.bodies.len()];
        for &i in &read.bodies {
            *is_read.get_mut(i)? = true;
        }
        let mut names = std::collections::HashSet::with_capacity(before.bodies.len());
        let mut next = after.bodies.iter().peekable();
        let mut fates = Vec::with_capacity(read.bodies.len());
        let mut changed = false;
        for (x, read) in before.bodies.iter().zip(is_read) {
            // A name twice (a damaged state) does not say which body a read meant.
            if !names.insert(x.name.as_str()) {
                return None;
            }
            let same = |b: &ModelBody| b.key == x.key && b.body.same(&x.body);
            if !read {
                next.next().filter(|b| b.name == x.name && same(b))?;
                continue;
            }
            let fate = match next.next_if(|b| b.name == x.name) {
                Some(b) if same(b) => Fate::Kept,
                Some(b) => Fate::Changed(Box::new(b.clone())),
                None => Fate::Removed,
            };
            changed |= !matches!(fate, Fate::Kept);
            fates.push((x.name.clone(), fate));
        }
        let made: Vec<ModelBody> = next.cloned().collect();
        // Each in its own slot: a body numbered from the counter can come out with a name stored
        // in another.
        let stored = |(k, b): (usize, &ModelBody)| f.body_names.get(k) == Some(&b.name) && !names.contains(b.name.as_str());
        if !made.is_empty() && (changed || !made.iter().enumerate().all(stored)) {
            return None;
        }
        // Nor is it one that bodies it did not read have a say in.
        let changes = fates.iter().filter_map(|(_, fate)| match fate {
            Fate::Changed(b) => Some(b.as_ref()),
            Fate::Kept | Fate::Removed => None,
        });
        if unnamed_edges(f) || !made.iter().chain(changes).all(|b| names_alone(b, f)) {
            return None;
        }
        // Counted only by a stored `BodyN` name: a body numbered from the counter is named by
        // every body before it.
        let number = |b: &ModelBody| b.name.strip_prefix("Body").filter(|_| crate::is_counted_body_name(&b.name))?.parse::<usize>().ok();
        let counter = match (after.body_counter, made.iter().filter_map(number).max()) {
            (c, Some(n)) if c == n && n > before.body_counter => n,
            (c, _) if c != before.body_counter => return None,
            (c, Some(n)) if n <= c => {
                // Counted already here, but maybe not where it is made again: only a tool's new
                // body is known to count ([`new_name`]).
                if !independent_solid(f) {
                    return None;
                }
                n
            }
            _ => 0,
        };
        Some(Relative { read: fates, made, counter })
    }
}

/// Whether blend `f` finds its edges by name on any body ([`named_body`]), not on one it names.
fn unnamed_edges(f: &Feature) -> bool {
    matches!(&f.kind, FeatureKind::Fillet { body: None, .. } | FeatureKind::Chamfer { body: None, .. }) && f.edge_names.iter().any(|n| !n.is_empty())
}

/// A solid of its own: its faces are never named after the bodies it lies on.
fn independent_solid(f: &Feature) -> bool {
    let kind = &f.kind;
    let solid =
        numeric_tool(kind) && !matches!(kind, FeatureKind::Coil { .. }) || matches!(kind, FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. });
    solid && feature_operation(f) == Some(Operation::NewBody)
}

/// Whether body `b`, as feature `f` made or changed it, names its faces from its own history: a
/// face named from geometry is named after whatever bodies it lies on
/// ([`crate::naming::geometric_names`]), also ones the feature did not read. The names a saved
/// model kept were worked out when it was made.
fn names_alone(b: &ModelBody, f: &Feature) -> bool {
    use crate::lineage::{Lineage, Operand};
    fn exact(l: &Lineage) -> bool {
        l.history.faces.iter().all(|face| !face.origins.is_empty())
            && l.operands.iter().all(|operand| match operand.as_ref() {
                Operand::Made(roles) | Operand::CopiedTool { roles, .. } | Operand::Rebuilt { roles, .. } => roles.iter().all(|r| !r.is_empty()),
                Operand::Grown { faces, .. } => faces.iter().all(Option::is_some),
                Operand::Stage(stage) => exact(&stage.lineage),
                Operand::Body(_) | Operand::Copy { .. } | Operand::Inner(_) => true,
            })
    }
    if matches!(f.kind, FeatureKind::Move { .. }) || independent_solid(f) {
        return true;
    }
    b.lineage().map_or(b.names.is_saved(), |l| exact(&l))
}

impl ModelState {
    /// How feature `feature`, which read the bodies `read` of `before`, took it to this state
    /// (not stamped yet); `None` when it changed anything else: a body it did not read, a sketch
    /// other than its own, the construction, threads or blend records.
    fn change_from(&self, before: &ModelState, f: &Feature, read: &Reads) -> Option<Change> {
        let feature = f.id;
        let (kept, sketch) = match self.sketches.split_last() {
            Some((last, rest)) if last.feature == feature => (rest, Some(last.clone())),
            _ => (self.sketches.as_slice(), None),
        };
        let others = before.sketches.iter().filter(|s| s.feature != feature);
        let same_rest = self.surface_sheets.is_empty()
            && kept.len() == others.clone().count()
            && kept.iter().zip(others).all(|(a, b)| Arc::ptr_eq(a, b))
            && (sketch.is_some() || kept.len() == before.sketches.len())
            && self.sketch_keys == before.sketch_keys
            && self.construct == before.construct
            && others_same(&self.blend_edges, &before.blend_edges, feature)
            && others_same(&self.fitted, &before.fitted, feature)
            && self.tapered.keys().filter(|k| **k != feature).eq(before.tapered.keys().filter(|k| **k != feature))
            && others_same(&self.kept_separate, &before.kept_separate, feature)
            && others_same(&self.pattern_left_out, &before.pattern_left_out, feature)
            && others_same(&self.named_faces, &before.named_faces, feature)
            && serde_json::to_string(&self.threads).ok() == serde_json::to_string(&before.threads).ok();
        if !same_rest {
            return None;
        }
        // Bodies passed through keep their order: each is looked for after the last one found.
        let mut from = 0;
        let mut untouched = vec![false; before.bodies.len()];
        let mut bodies = Vec::with_capacity(self.bodies.len());
        for b in &self.bodies {
            let at = before.bodies.iter().enumerate().skip(from).find(|(_, x)| x.name == b.name).map(|(j, _)| j);
            match at.filter(|&j| before.bodies.get(j).is_some_and(|x| x.key == b.key && x.body.same(&b.body))) {
                Some(j) => {
                    bodies.push(Changed::Kept(j));
                    if let Some(u) = untouched.get_mut(j) {
                        *u = true;
                    }
                    from = j + 1;
                }
                None => bodies.push(Changed::Made(Box::new(b.clone()))),
            }
        }
        let only_read = untouched.iter().enumerate().all(|(j, u)| *u || read.bodies.contains(&j));
        only_read.then(|| Change {
            bodies: bodies.into(),
            sketch,
            body_counter: self.body_counter,
            blend: self.blend_edges.get(&feature).cloned(),
            fitted: self.fitted.get(&feature).cloned(),
            tapered: self.tapered.get(&feature).cloned(),
            kept_separate: self.kept_separate.get(&feature).copied(),
            pattern_left_out: self.pattern_left_out.get(&feature).cloned(),
            named_faces: self.named_faces.get(&feature).cloned(),
            relative: Relative::of(before, self, f, read).map(Arc::new),
        })
    }

    /// This state with `change` (feature `feature`'s) made to it: its bodies, those it kept taken
    /// from here; `None` when this state does not line up with it.
    fn with_change(&self, change: &Change, feature: u64) -> Option<ModelState> {
        let bodies = change
            .bodies
            .iter()
            .map(|b| match b {
                Changed::Kept(j) => self.bodies.get(*j).map(|b| ModelBody { made: crate::lineage::Made::Before, ..b.clone() }),
                Changed::Made(b) => Some((**b).clone()),
            })
            .collect::<Option<Vec<_>>>()?;
        Some(self.with_changed_bodies(change, feature, bodies, change.body_counter))
    }

    /// This state with `change`'s relative form ([`Relative`]) made to it: the bodies it read,
    /// found by name, as it left them, every other body where it stands, and the bodies it made
    /// after them; `None` when it has none, or this state lacks a body it read or holds a name it
    /// made.
    fn with_relative(&self, change: &Change, feature: u64) -> Option<ModelState> {
        let relative = change.relative.as_deref()?;
        let fates: std::collections::HashMap<&str, &Fate> = relative.read.iter().map(|(n, fate)| (n.as_str(), fate)).collect();
        let mut found = 0;
        let mut bodies = Vec::with_capacity(self.bodies.len() + relative.made.len());
        for b in &self.bodies {
            let before = || ModelBody { made: crate::lineage::Made::Before, ..b.clone() };
            match fates.get(b.name.as_str()) {
                None => bodies.push(before()),
                Some(fate) => {
                    found += 1;
                    match fate {
                        Fate::Kept => bodies.push(before()),
                        Fate::Changed(c) => bodies.push((**c).clone()),
                        Fate::Removed => {}
                    }
                }
            }
        }
        // Each body it read once, and no body under a name it made.
        if found != relative.read.len() || relative.made.iter().any(|m| self.body(&m.name).is_some()) {
            return None;
        }
        bodies.extend(relative.made.iter().cloned());
        Some(self.with_changed_bodies(change, feature, bodies, self.body_counter.max(relative.counter)))
    }

    /// This state with `bodies` and the rest of `change` (feature `feature`'s) made to it.
    fn with_changed_bodies(&self, change: &Change, feature: u64, bodies: Vec<ModelBody>, body_counter: usize) -> ModelState {
        let mut sketches = self.sketches.clone();
        if let Some(s) = &change.sketch {
            sketches.retain(|s| s.feature != feature);
            sketches.push(s.clone());
        }
        let mut next = ModelState { bodies, sketches, body_counter, ..self.clone() };
        next.blend_edges.remove(&feature);
        next.fitted.remove(&feature);
        next.tapered.remove(&feature);
        if let Some(b) = &change.blend {
            next.blend_edges.insert(feature, b.clone());
        }
        if let Some(r) = &change.fitted {
            next.fitted.insert(feature, r.clone());
        }
        if let Some(t) = &change.tapered {
            next.tapered.insert(feature, t.clone());
        }
        next.kept_separate.remove(&feature);
        if let Some(k) = change.kept_separate {
            next.kept_separate.insert(feature, k);
        }
        next.pattern_left_out.remove(&feature);
        if let Some(l) = &change.pattern_left_out {
            next.pattern_left_out.insert(feature, l.clone());
        }
        next.named_faces.remove(&feature);
        if let Some(p) = &change.named_faces {
            next.named_faces.insert(feature, p.clone());
        }
        next
    }
}

/// Whether `a` and `b` hold the same records for every feature but `feature`.
fn others_same<V: PartialEq>(a: &BTreeMap<u64, V>, b: &BTreeMap<u64, V>, feature: u64) -> bool {
    a.iter().filter(|(k, _)| **k != feature).eq(b.iter().filter(|(k, _)| **k != feature))
}

/// The frame of an occurrence path from the root (its occurrences' transforms in turn); `None`
/// when the path no longer leads anywhere.
fn path_frame(doc: &Document, path: &[u64]) -> Option<Mat> {
    if path.len() > crate::MAX_PLACEMENT_DEPTH + 1 {
        return None;
    }
    let (mut m, mut c) = (crate::IDENTITY, 0);
    for id in path {
        let o = doc.occurrences.iter().find(|o| o.id == *id && o.parent == c)?;
        m = crate::mat_mul(&m, &o.transform);
        c = o.component;
    }
    Some(m)
}

/// Hash a placement on a grid far finer than anything a design shows ([`QUANTUM`] mm, a
/// thousandth of that for the rotation), so the same relative placement reached through
/// different whole-assembly motions (each rounding a little differently) reads the same.
fn hash_placement(m: &Mat, h: &mut impl Hasher) {
    for (col, column) in m.iter().enumerate() {
        let q = if col == 3 { QUANTUM } else { QUANTUM * 1e-3 };
        for v in column {
            let r = (v / q).round();
            // -0 and 0 are the same place.
            (if r == 0.0 { 0.0f64 } else { r }).to_bits().hash(h);
        }
    }
}

/// [`placement_digest`] of a pattern's or mirror's copies of sources kept in another frame than
/// its own ([`kept_in`], #19451): each copy's transform as it acts there, where that is not the
/// transform itself, and for picks where every body is placed, which decides the body a copy lands
/// on. Whether there was such a source.
fn copy_frames_digest(doc: &Document, f: &Feature, features: &[FeatureId], st: &ModelState, reads: &mut PlacementReads, h: &mut impl Hasher) -> bool {
    let own = f.component;
    let sources = replayed_sources(doc, features);
    if !sources.iter().any(|g| kept_in(doc, st, own, g).is_some()) {
        return false;
    }
    let vals = evaluation_values(doc).0;
    let Ok(plan) = own_copies(doc, &vals, st, f).and_then(|mats| copy_plan(doc, &vals, st, own, features, &mats)) else {
        reads.all = true;
        return true;
    };
    /// Each of `mats` as it acts in the frame at `at`, where it acts otherwise there.
    fn moved(id: u64, mats: &[Mat], at: &Mat, h: &mut impl Hasher) -> Result<()> {
        let placed = |m: &Mat| {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            hash_placement(m, &mut h);
            h.finish()
        };
        for (k, m) in mats.iter().enumerate() {
            let there = moved_in(m, at)?;
            if placed(&there) != placed(m) {
                (id, k).hash(h);
                hash_placement(&there, h);
            }
        }
        Ok(())
    }
    for (src, mats, _) in &plan {
        if let Some(at) = kept_in(doc, st, own, src) {
            reads.all |= moved(src.id, mats, &at, h).is_err();
        }
    }
    // A pattern's or mirror's own copies are composed in this one's frame (see `copy_plan`).
    for g in &sources {
        if let (Some(back), Ok(Some((_, inner)))) =
            (component_in(doc, own, g.component).and_then(|at| crate::mat_inverse(&at)), copied_features(doc, &vals, st, g))
        {
            reads.all |= moved(g.id, &inner, &back, h).is_err();
        }
    }
    if plan.iter().any(|(src, _, _)| tools_component(doc, src).is_none() && kept_in(doc, st, own, src).is_some()) {
        reads.components.extend(doc.body_owners(st));
    }
    true
}

/// [`placement_digest`] of a mirror of placed bodies: each placement it was picked at (the copy's
/// component, that component's frame and the frame it was picked in, both in its own component's
/// frame as [`mirror_copy_frame`] gives them) and the offsets of the bodies it mirrors (#1938).
/// It mirrors in a plane of its own component by just those frames, so a joint that moves its own
/// part together with what it mirrors leaves it, bit for bit, as it was (#14271). A plane read from
/// model geometry, or a component origin datum, can lie in another part, so its own component's
/// world frame counts then too.
fn mirror_placements_digest(doc: &Document, f: &Feature, plane: &PlaneRef, placements: &[Vec<u64>], bodies: &[crate::BodyName]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if !placement_reuse::local_plane(plane, 0) {
        doc.component_transform(f.component).iter().flatten().for_each(|x| x.to_bits().hash(&mut h));
    }
    // A feature added after the mirror that builds on its copies needs them made for that.
    placement_reuse::copies_only_shown(doc, f).hash(&mut h);
    for placed in placements.iter().map(|path| mirror_copy_frame(doc, f.component, path)) {
        placed.is_some().hash(&mut h);
        if let Some((component, frame, picked)) = placed {
            component.hash(&mut h);
            frame.iter().chain(&picked).flatten().for_each(|x| x.to_bits().hash(&mut h));
        }
    }
    for b in bodies {
        serde_json::to_string(&doc.body_offsets.get(&b.0)).unwrap_or_default().hash(&mut h);
    }
    h.finish()
}

/// What a body pattern's picked placements read: each placement's frame and the component its
/// copies join, both in the feature's component, and each body's offset.
fn pattern_placements_digest(doc: &Document, f: &Feature, placements: &[Vec<u64>], bodies: &[crate::BodyName]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for placed in placements.iter().map(|path| mirror_copy_frame(doc, f.component, path)) {
        placed.is_some().hash(&mut h);
        if let Some((component, frame, picked)) = placed {
            component.hash(&mut h);
            frame.iter().chain(&picked).flatten().for_each(|x| x.to_bits().hash(&mut h));
        }
    }
    for b in bodies {
        serde_json::to_string(&doc.body_offsets.get(&b.0)).unwrap_or_default().hash(&mut h);
    }
    h.finish()
}

/// Whether only the bodies `f` makes follow where parts are placed: a mirror of placed bodies
/// into new bodies. A feature after it reads its copies only by naming them, so a pose change
/// need not evaluate features that don't (#12139).
fn pose_only(f: &Feature) -> bool {
    matches!(&f.kind, FeatureKind::Mirror { features, bodies, placements, combine: false, .. }
        if features.is_empty() && !bodies.is_empty() && !placements.is_empty())
}

/// The new bodies of pose-only `f`, evaluated again (to `state`) for its pose alone: the result
/// `old` was on the same definitions and made bodies of the same names, and nothing besides.
fn pose_moved(old: Option<&FeatureResult>, f: &Feature, fp: u64, state: &ModelState) -> Option<Vec<ModelBody>> {
    let old = old.filter(|o| o.id == f.id && o.fingerprint == fp && !o.stale && !o.skipped && !o.unkept && o.error.is_none())?;
    let made = |st: &ModelState| st.bodies.iter().filter(|b| b.feature == f.id).cloned().collect::<Vec<_>>();
    let (before, after) = (made(&old.state), made(state));
    let names = |bodies: &[ModelBody]| bodies.iter().map(|b| b.name.clone()).collect::<std::collections::BTreeSet<_>>();
    let same = names(&before) == names(&after)
        && old.state.bodies.len() == state.bodies.len()
        && old.state.threads.len() == state.threads.len()
        && old.state.construct.len() == state.construct.len();
    same.then_some(after)
}

/// `old` (a result after the moved pose-only features) with their new bodies in place of theirs.
fn with_moved(old: &Arc<ModelState>, moved: &[(u64, Vec<ModelBody>)]) -> Arc<ModelState> {
    let mut st = (**old).clone();
    for b in &mut st.bodies {
        if let Some(new) = moved.iter().filter(|(id, _)| *id == b.feature).flat_map(|(_, bodies)| bodies).find(|n| n.name == b.name) {
            *b = new.clone();
        }
    }
    Arc::new(st)
}

/// Whether `f` (starting from `st`) reads none of the bodies of `moved`: it reads no bodies, or
/// only bodies it names or would cut or join, none of them those. Anything else (a picked face,
/// a projection, To Object, ...) is taken as reading them.
fn reads_none_of(f: &Feature, st: &ModelState, moved: &[(u64, Vec<ModelBody>)], components: &std::collections::HashMap<u64, u64>) -> bool {
    if !f.face_names.is_empty() || !f.edge_names.is_empty() || !f.edge_refs.is_empty() {
        return false;
    }
    let reads: Vec<String> = match &f.kind {
        _ if placement_reuse::local_geometry(f, components) => Vec::new(),
        FeatureKind::Sketch { plane, sketch } if sketch.links.is_empty() && sketch.wires.is_empty() && placement_reuse::local_plane(plane, 0) => {
            Vec::new()
        }
        FeatureKind::Mirror { features, bodies, plane, .. } if features.is_empty() && placement_reuse::local_plane(plane, 0) => {
            bodies.iter().map(|b| b.0.clone()).collect()
        }
        FeatureKind::Extrude { extent, .. } if extent.to.is_some() || extent.from.is_some() => return false,
        FeatureKind::Revolve { to: Some(_), .. } | FeatureKind::Revolve { axis: AxisRef::Model { .. }, .. } => return false,
        kind if drawn_tools_change_bodies(kind) => {
            let mut reads = PlacementReads::default();
            reads.drawn_targets(Some(st), f);
            if reads.all {
                return false;
            }
            reads.bodies.into_iter().collect()
        }
        _ => return false,
    };
    !moved.iter().flat_map(|(_, bodies)| bodies).any(|b| reads.contains(&b.name))
}

/// The datum part of [`placement_digest`] for the arms that resolve before the shared read
/// loop runs: each datum path, and where its component sits relative to the reading
/// component's frame, so moving that placement changes the hash (`u8::MAX` for one that no
/// longer resolves). `0` when there is nothing to fold in, so a caller can XOR it unchanged.
fn datum_paths_digest(doc: &Document, f: &Feature, datums: &[Vec<u64>]) -> u64 {
    if datums.is_empty() {
        return 0;
    }
    let Some(inverse) = crate::mat_inverse(&doc.component_transform(f.component)) else { return placements_hash(doc) };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for path in datums {
        path.hash(&mut h);
        match path_frame(doc, path) {
            Some(m) => hash_placement(&crate::mat_mul(&inverse, &m), &mut h),
            None => u8::MAX.hash(&mut h),
        }
    }
    h.finish()
}

/// The component origin datums a plane is built on, wherever they are nested (an offset of a
/// datum, a perpendicular to a datum axis): each is a placement the plane reads (#7313).
fn plane_datum_paths(p: &PlaneRef, out: &mut Vec<Vec<u64>>) {
    match p {
        PlaneRef::Datum { datum } => out.push(datum.path.clone()),
        PlaneRef::Origin { .. } | PlaneRef::Custom { .. } | PlaneRef::Placed { .. } | PlaneRef::Face { .. } | PlaneRef::Construction { .. } => {}
        PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } => plane_datum_paths(base, out),
        PlaneRef::AtAngle { base, line, pivot, .. } => {
            plane_datum_paths(base, out);
            if let Some(g) = line {
                geo_datum_paths(g, out);
            }
            if let Some(g) = pivot {
                geo_datum_paths(g, out);
            }
        }
        PlaneRef::ThroughPoint { base, point } => {
            plane_datum_paths(base, out);
            geo_datum_paths(point, out);
        }
        PlaneRef::Midplane { a, b, .. }
        | PlaneRef::TwoEdges { a, b }
        | PlaneRef::AxisPoint { axis: a, point: b }
        | PlaneRef::Perpendicular { plane: a, line: b }
        | PlaneRef::SquareToLine { line: a, point: b } => {
            geo_datum_paths(a, out);
            geo_datum_paths(b, out);
        }
        PlaneRef::ThreePoints { a, b, c } => {
            geo_datum_paths(a, out);
            geo_datum_paths(b, out);
            geo_datum_paths(c, out);
        }
        PlaneRef::Tangent { face, reference, .. } => {
            geo_datum_paths(face, out);
            if let Some(r) = reference {
                geo_datum_paths(r, out);
            }
        }
        PlaneRef::AlongPath { path, .. } => geo_datum_paths(path, out),
    }
}

/// The component origin datums a construct reference is built on, wherever they are nested
/// (a plane on a datum axis, an axis line through a datum plane): each is a placement the
/// feature reads, so the feature is found again when that component moves (#7313).
fn geo_datum_paths(g: &crate::construct::GeoRef, out: &mut Vec<Vec<u64>>) {
    match g {
        crate::construct::GeoRef::Datum { datum } => out.push(datum.path.clone()),
        crate::construct::GeoRef::Plane { plane } => plane_datum_paths(plane, out),
        _ => {}
    }
}

/// Fingerprint the exact transforms used by the two supports. Captured placements keep their
/// geometry when an occurrence moves, but still depend on valid paths and live body offsets.
fn gusset_placements_digest(doc: &Document, f: &Feature, st: Option<&ModelState>, supports: &[crate::gusset::Support]) -> u64 {
    let [first, second] = supports else { return 0 };
    let Some(st) = st else { return placements_hash(doc) };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for support in [first, second] {
        let PlaneRef::Face { anchor: Some(anchor), .. } = &support.plane else { return 0 };
        let Some(body) = st.body(&anchor.target.body) else {
            0u8.hash(&mut h);
            continue;
        };
        match anchor.source_to_author(doc, f.component, body) {
            Ok(transform) => {
                1u8.hash(&mut h);
                hash_placement(&transform, &mut h);
            }
            Err(err) => {
                2u8.hash(&mut h);
                err.to_string().hash(&mut h);
            }
        }
    }
    h.finish() | 1
}

/// Where the geometry a feature reads from other components is placed, as its own component
/// sees it: the placement of each component (or occurrence path) it reads relative to its own
/// frame. A feature that reads nothing outside its own frame gives 0, so moving parts around
/// (a joint driven, an occurrence moved) leaves it as it was; one that reads a part moved
/// relative to it (a sketch projecting a face of another part, To Object, a mirror of a placed
/// body) is evaluated again. `st` is the model the feature starts from, which says what
/// component a body it names belongs to (without it, every component is taken as read).
fn placement_digest(doc: &Document, f: &Feature, st: Option<&ModelState>) -> u64 {
    use crate::walk::Walk;
    if let FeatureKind::FrameGusset { supports, .. } = &f.kind {
        return gusset_placements_digest(doc, f, st, supports);
    }
    // No shortcut for a design without placements: its first component (placed where the
    // design is) must not change what its features read, or creating it evaluates them all.
    let mut reads = PlacementReads::default();
    let mut frame = None;
    let mut copied = false;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let mut has_captured_profile = false;
    // A sketch, profile or path of another component is moved through both placements, unless the
    // feature captured its component's frame at creation time (in which case only the sketch's
    // component motion relative to their common ancestor affects the local profile).
    if let Some(consumer_in_lca) = f.profile_frame
        && let Some(sketch) = f.kind.constructor_profile_sketch()
    {
        let owner = doc.feature(sketch).map(|s| s.component).unwrap_or(0);
        let lca = doc.common_ancestor(f.component, owner);
        let owner_in_lca = doc.component_transform_in(owner, lca);
        "captured profile frame".hash(&mut h);
        sketch.hash(&mut h);
        hash_placement(&consumer_in_lca, &mut h);
        hash_placement(&owner_in_lca, &mut h);
        has_captured_profile = true;
    } else {
        for (sketch, _) in f.kind.profile_sels() {
            if doc.feature(sketch).is_some_and(|s| s.component != f.component) {
                reads.sketch(doc, sketch);
                // A later transfer can preserve this profile's historical frame. Changes to that
                // boundary (including suppression and rollback) must invalidate the constructor.
                if let Some(relative) = crate::body_transfer::profile_frame(doc, f, sketch) {
                    "transferred profile".hash(&mut h);
                    sketch.hash(&mut h);
                    hash_placement(&relative, &mut h);
                }
            }
        }
    }
    let path = match &f.kind {
        FeatureKind::Sweep { path_sketch, .. }
        | FeatureKind::SurfaceSweep { path_sketch, .. }
        | FeatureKind::Pipe { path_sketch, .. }
        | FeatureKind::Pattern { pattern: crate::PatternKind::Path { path_sketch, .. }, .. } => Some(*path_sketch),
        FeatureKind::FrameMember { path_sketch, .. } => *path_sketch,
        _ => None,
    };
    let wired = |id: &crate::SketchId| doc.sketch(id.0).is_ok_and(|sketch| !sketch.wires.is_empty());
    let rails: &[crate::SweepRail] = match &f.kind {
        FeatureKind::Sweep { rails, .. }
        | FeatureKind::SurfaceSweep { rails, .. }
        | FeatureKind::Loft { rails, .. }
        | FeatureKind::SurfaceLoft { rails, .. } => rails,
        _ => &[],
    };
    // Members map their frame sketch into their own component's frame, 2D lines included.
    let member = matches!(f.kind, FeatureKind::FrameMember { .. });
    // A feature that captured its component's frame reads the components it names through that
    // frame, so their placement still counts (the path of a sweep, the rails of any of them).
    if path.as_ref().is_some_and(|p| member || wired(p) || has_captured_profile) || rails.iter().any(|r| wired(&r.sketch) || has_captured_profile) {
        if let Some(path) = path {
            reads.sketch(doc, path.0);
        }
        // A sweep's profile and the rails' components place them too.
        let profile = match &f.kind {
            FeatureKind::Sweep { sketch, .. } | FeatureKind::SurfaceSweep { sketch, .. } => Some(sketch),
            _ => None,
        };
        for sketch in profile.into_iter().chain(rails.iter().map(|r| &r.sketch)) {
            reads.sketch(doc, sketch.0);
        }
    }
    // A network through 3D sketch curves reads their points through the components the
    // feature and those sketches are placed in, as a sweep's wired path does.
    if let FeatureKind::SurfaceNetwork { u, v, .. } = &f.kind {
        let sketches: Vec<crate::SketchId> = u.iter().chain(v).filter_map(|c| c.sketch).collect();
        if sketches.iter().any(wired) {
            for sketch in &sketches {
                reads.sketch(doc, sketch.0);
            }
        }
    }
    // The faces To / From Object names, in `face_points` order.
    let object_names = match &f.kind {
        FeatureKind::Extrude { extent, .. } => usize::from(extent.to.is_some()) + usize::from(extent.from.is_some()),
        FeatureKind::Revolve { to, .. } => usize::from(to.is_some()),
        _ => 0,
    };
    // A component origin datum is found again wherever its component is placed now (#7313): the
    // feature reads that placement, the way a face anchor re-finds its face. Collected from
    // every reference the feature holds, whichever kind carries it — before the kind match,
    // because a mirror with picked placements returns from it before the read loop runs.
    let mut datums: Vec<Vec<u64>> = Vec::new();
    f.kind.walk_geometry(&mut |g| match g {
        crate::feature_geometry::Geo::Plane(p) => plane_datum_paths(p, &mut datums),
        crate::feature_geometry::Geo::Construct(g) => geo_datum_paths(g, &mut datums),
        _ => {}
    });
    match &f.kind {
        // To / From Object finds its target where the target's body is placed (#928): only that
        // body's placement, not every part's (#6325). Its tools meet their targets as other
        // joins and cuts do.
        kind if object_names > 0 => {
            reads.object_faces(st, f.face_names.get(..object_names));
            if drawn_tools_change_bodies(kind) {
                reads.drawn_targets(st, f);
            }
        }
        // A mirror of placed bodies follows the placements it was picked at, as its own component
        // sees them, not every occurrence: moving an unrelated one, or its own part together with
        // them, leaves it (and its cache) alone.
        FeatureKind::Mirror { placements, bodies, plane, .. } if !placements.is_empty() => {
            // The mirror's plane can hold a datum: that datum's component's placement is read
            // too, or moving the component serves a stale mirror from the cache (#7313 review).
            // The shared read loop below never runs for picked placements, so the datum frames
            // fold in here, each relative to the reading component, as `reads.paths` does.
            return mirror_placements_digest(doc, f, plane, placements, bodies) ^ datum_paths_digest(doc, f, &datums);
        }
        FeatureKind::MeshReduce { body, .. } => {
            reads.placed_body(st, body);
            if let Some(st) = st {
                let name = copy_body_name(st, f, 0, &format!("{body} Reduced"));
                serde_json::to_string(&doc.body_offsets.get(&name)).unwrap_or_default().hash(&mut h);
            } else {
                reads.all = true;
            }
        }
        // A component's envelope bounds what is placed inside it, where it stands in its frame:
        // moving a part inside it builds it again, moving the component does not.
        FeatureKind::BoundingSolid { contents: true, .. } => return contents_digest(doc, f, st),
        // A combine moves tools placed differently from its target into the target's frame (#3383).
        FeatureKind::Combine { target, tools, .. } => {
            frame = st.and_then(|st| st.body(target)).map(|b| body_world(doc, b));
            reads.placed_body(st, target);
            // A tool from a further placement (`Bit (Drill:2)`) is read through its occurrence
            // path alone: reading every placement cut it again for each occurrence added anywhere.
            let copies = st.map(|st| copy_paths(doc, st, tools)).unwrap_or_default();
            for tool in tools {
                match copies.get(&**tool) {
                    Some(path) => reads.paths.push(path.clone()),
                    None => reads.placed_body(st, tool),
                }
            }
        }
        // A plane taken from a face, or tool faces used as they are (#9305), is moved from the
        // tool's frame into the body's.
        FeatureKind::Split { body, tool: Some(tool), tool_plane, tool_sheet, .. } if *tool_plane || !tool_sheet.is_extended() => {
            frame = st.and_then(|st| st.body(body)).map(|b| body_world(doc, b));
            reads.placed_body(st, body);
            reads.placed_body(st, &tool.body);
        }
        // A pattern of bodies copies them along its directions or about its axis in its own
        // component's frame, so it follows where each body it copies is placed relative to that
        // frame: copies of a root block, patterned in a component, move with the component.
        FeatureKind::Pattern { bodies, placements, .. } if !bodies.is_empty() => {
            // Bodies picked where a placement shows them follow those placements, as a mirror's.
            if !placements.is_empty() {
                pattern_placements_digest(doc, f, placements, bodies).hash(&mut h);
            }
            for b in bodies {
                reads.placed_body(st, &b.0);
            }
        }
        // Copies meet the bodies they change where those are placed relative to their source's
        // tools' frame, whatever the pattern's own placement (#14271).
        FeatureKind::Pattern { features, bodies, .. } | FeatureKind::Mirror { features, bodies, .. } if bodies.is_empty() => {
            for src in replayed_sources(doc, features) {
                let Some(frame) = tools_component(doc, src).map(|c| doc.component_transform(c)) else { continue };
                let mut each = PlacementReads::default();
                match (st, crate::mat_inverse(&frame)) {
                    (Some(st), Some(inverse)) => {
                        let (_, targets, scope) = copy_targets(st, f, src);
                        each.targets(st, &scope, &targets);
                        hash_placed_bodies(doc, Some(st), &each.bodies, &inverse, &mut h);
                        copied |= !each.bodies.is_empty();
                    }
                    _ => each.all = true,
                }
                reads.all |= each.all;
                reads.unlisted |= each.unlisted;
            }
            match st {
                Some(st) => copied |= copy_frames_digest(doc, f, features, st, &mut reads, &mut h),
                None => reads.all |= replayed_sources(doc, features).iter().any(|g| component_in(doc, f.component, g.component).is_some()),
            }
        }
        // Joins, cuts and intersects meet their targets where those are placed (#3383).
        kind if drawn_tools_change_bodies(kind) => reads.drawn_targets(st, f),
        // So does a hole at sketch points, in its sketch's frame (#17623).
        FeatureKind::Hole { points: Some(sp), .. } => {
            frame = doc.feature(sp.sketch.0).map(|s| doc.component_transform(s.component));
            reads.drawn_targets(st, f);
        }
        // And a rib, which is built against the bodies where its sketch is drawn (#17655).
        FeatureKind::Rib { sketch, .. } => {
            frame = doc.feature(sketch.0).map(|s| doc.component_transform(s.component));
            reads.drawn_targets(st, f);
        }
        FeatureKind::FramePlate { plane, .. } => {
            if let Some(anchor) = crate::face_anchor::plane_face_anchor(plane) {
                match anchor.author_frame(doc, f.component) {
                    Ok(m) => frame = Some(m),
                    Err(e) => e.to_string().hash(&mut h),
                }
                if anchor.path.is_empty() {
                    reads.components.insert(anchor.component);
                } else {
                    reads.paths.push(anchor.path.clone());
                }
                reads.placed_body(st, &anchor.target.body);
            }
        }
        FeatureKind::Sketch { plane, sketch } => {
            match crate::face_anchor::plane_face_anchor(plane) {
                // Read in the frame of the placement it was drawn in, its face through the
                // placement it was picked at.
                Some(anchor) => {
                    match anchor.author_frame(doc, f.component) {
                        Ok(m) => frame = Some(m),
                        Err(e) => e.to_string().hash(&mut h),
                    }
                    if anchor.path.is_empty() {
                        reads.components.insert(anchor.component);
                    } else {
                        reads.paths.push(anchor.path.clone());
                    }
                }
                // A face or edges picked on whatever body lies there.
                None if plane_reads_geometry(plane) => reads.all = true,
                None => {}
            }
            for link in &sketch.links {
                reads.link(doc, st, &link.source);
            }
        }
        // Each end is read as a sketch on a face is: in the frame it was capped in, its member
        // through the placement it was picked at, so moving that occurrence rebuilds the cap.
        FeatureKind::FrameEndCap { ends, .. } => {
            for anchor in ends.iter().filter_map(|end| crate::face_anchor::plane_face_anchor(&end.plane)) {
                match anchor.author_frame(doc, f.component) {
                    Ok(m) => {
                        hash_placement(&m, &mut h);
                        frame.get_or_insert(m);
                    }
                    Err(e) => e.to_string().hash(&mut h),
                }
                if anchor.path.is_empty() {
                    reads.components.insert(anchor.component);
                } else {
                    reads.paths.push(anchor.path.clone());
                }
            }
        }
        // A trim's face targets are read as an end cap's faces are: in the frame each was picked
        // in, through the placement it was picked at, so moving that occurrence trims again.
        FeatureKind::FrameTrim { to, .. } => {
            let faces = to.iter().filter_map(|t| match t {
                crate::frame::TrimTarget::Plane { plane } => crate::face_anchor::plane_face_anchor(plane),
                crate::frame::TrimTarget::Member { .. } => None,
            });
            for anchor in faces {
                match anchor.author_frame(doc, f.component) {
                    Ok(m) => {
                        hash_placement(&m, &mut h);
                        frame.get_or_insert(m);
                    }
                    Err(e) => e.to_string().hash(&mut h),
                }
                if anchor.path.is_empty() {
                    reads.components.insert(anchor.component);
                } else {
                    reads.paths.push(anchor.path.clone());
                }
            }
        }
        _ => {}
    }
    // A Frame Member's pairs join points on bodies, sketches and construction points of any
    // component, found again in its own frame (#15705): moving any of them moves the member.
    // ponytail: read as everything, so any placement change rebuilds it; collect the bodies and
    // sketches its pairs name if that shows in a large assembly.
    if matches!(&f.kind, FeatureKind::FrameMember { pairs, .. } if !pairs.is_empty()) {
        reads.all = true;
    }
    reads.paths.extend(datums);
    if reads.is_empty() && !copied {
        return if has_captured_profile { h.finish() } else { 0 };
    }
    let frame = frame.unwrap_or_else(|| doc.component_transform(f.component));
    let Some(inverse) = crate::mat_inverse(&frame) else { return placements_hash(doc) };
    if reads.all {
        reads.components.insert(0);
        match st {
            // The components holding what the feature can reach: a component made later, or one
            // that holds nothing yet (just created), changes nothing it reads.
            Some(st) => {
                reads.components.extend(doc.body_owners(st));
                let owner = |id: u64| doc.feature(id).map(|g| g.component);
                reads.components.extend(st.sketches.iter().filter_map(|s| owner(s.feature)));
                reads.components.extend(st.construct.iter().filter_map(|c| owner(c.feature)));
            }
            None => reads.components.extend(doc.components.iter().map(|c| c.id)),
        }
    }
    for c in &reads.components {
        c.hash(&mut h);
        // Its own component is read in its own frame.
        if *c != f.component {
            hash_placement(&crate::mat_mul(&inverse, &doc.component_transform(*c)), &mut h);
        }
    }
    for path in &reads.paths {
        path.hash(&mut h);
        match path_frame(doc, path) {
            Some(m) => hash_placement(&crate::mat_mul(&inverse, &m), &mut h),
            None => u8::MAX.hash(&mut h),
        }
    }
    hash_placed_bodies(doc, st, &reads.bodies, &inverse, &mut h);
    if reads.unlisted {
        placements_hash(doc).hash(&mut h);
    }
    // Legacy and component-wide reads still need the conservative offset dependency. Body
    // reads above already include just their own offsets, in the frame that evaluates them.
    if reads.all || !reads.components.is_empty() || !reads.paths.is_empty() {
        serde_json::to_string(&doc.body_offsets).unwrap_or_default().hash(&mut h);
    }
    // Never 0: the feature reads placements.
    h.finish() | 1
}

/// The bodies part of [`placement_digest`]: each body read, in name order, and where it is placed
/// relative to the reading frame `inverse` (`u8::MAX` for one not in `st`), as [`body_world`]
/// places it. The bodies, their components and the components' frames are looked up once: a
/// join or cut without targets reads every body of its component, and searching the model, the
/// timeline and the placements again for each of them made every evaluation, even one served
/// from the cache, quadratic in the bodies.
fn hash_placed_bodies(doc: &Document, st: Option<&ModelState>, names: &std::collections::BTreeSet<String>, inverse: &Mat, h: &mut impl Hasher) {
    let (Some(st), false) = (st, names.is_empty()) else {
        names.iter().for_each(|name| (name, u8::MAX).hash(h));
        return;
    };
    // The first body of a name, as `ModelState::body` finds it.
    let mut index: std::collections::HashMap<&str, usize> = std::collections::HashMap::with_capacity(st.bodies.len());
    for (i, b) in st.bodies.iter().enumerate() {
        index.entry(b.name.as_str()).or_insert(i);
    }
    let components = doc.model_body_components(st);
    let mut frames: std::collections::HashMap<u64, Mat> = std::collections::HashMap::new();
    for name in names {
        name.hash(h);
        let found = index.get(name.as_str()).and_then(|&i| Some((st.bodies.get(i)?, *components.get(i)?)));
        match found {
            Some((b, component)) => {
                let m = *frames.entry(component).or_insert_with(|| doc.component_transform(component));
                let world = doc.model_body_offset(b).map(|offset| crate::mat_mul(&m, offset)).unwrap_or(m);
                hash_placement(&crate::mat_mul(inverse, &world), h);
            }
            None => u8::MAX.hash(h),
        }
    }
}

/// [`placement_digest`] of a component's envelope: each placement inside its component, relative
/// to that component, and the bodies each holds with their offsets. Never 0.
fn contents_digest(doc: &Document, f: &Feature, st: Option<&ModelState>) -> u64 {
    let Some(st) = st else { return placements_hash(doc) | 1 };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    match crate::envelope::placed_contents(doc, st, f.component) {
        Ok(placed) => {
            for (b, m) in placed {
                b.name.hash(&mut h);
                hash_placement(&m, &mut h);
            }
        }
        Err(e) => e.to_string().hash(&mut h),
    }
    h.finish() | 1
}

/// The occurrence path of each name in `names` that `st` lacks and that names a further
/// placement's body (`Bit (Drill:2)`), found as [`with_placed_copies`] finds it.
fn copy_paths<'n>(doc: &Document, st: &ModelState, names: &'n [crate::BodyName]) -> std::collections::HashMap<&'n str, Vec<u64>> {
    let mut found = std::collections::HashMap::new();
    let missing: Vec<&str> = names.iter().map(|n| &**n).filter(|n| !n.is_empty() && st.body(n).is_none()).collect();
    if missing.is_empty() || doc.occurrences.is_empty() {
        return found;
    }
    let mut by_component: std::collections::HashMap<u64, Vec<&ModelBody>> = std::collections::HashMap::new();
    for (b, owner) in st.bodies.iter().zip(doc.model_body_components(st)) {
        by_component.entry(owner).or_default().push(b);
    }
    let world = crate::WorldNames::new(doc, st);
    for (comp, path, _) in doc.placements() {
        for b in by_component.get(&comp).into_iter().flatten() {
            let name = world.name(&b.name, comp, &path);
            if let Some(n) = missing.iter().find(|n| **n == name) {
                found.entry(*n).or_insert_with(|| path.clone());
            }
        }
    }
    found
}

/// Every occurrence's placement and every body offset (a feature read from a frame that cannot
/// be inverted).
fn placements_hash(doc: &Document) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for o in &doc.occurrences {
        o.id.hash(&mut h);
        o.transform.iter().flatten().for_each(|x| x.to_bits().hash(&mut h));
    }
    serde_json::to_string(&doc.body_offsets).unwrap_or_default().hash(&mut h);
    h.finish()
}

fn now() -> web_time::Instant {
    web_time::Instant::now()
}

impl Model {
    pub fn new() -> Self {
        Model::default()
    }

    /// Let `evaluate` return without waiting for the SMLib checks of the bodies its last features
    /// keep (the checks under [`kernel::Validation::EachFeature`] that run beside the features
    /// after them): the results show at once and the checks end beside the caller, which settles
    /// them ([`Model::settle_feature_checks`]). Off (the default), `evaluate` waits for every
    /// check, as before. The verdicts, results and refusals are the same either way: what a check
    /// refuses fails its feature in the next evaluation, as it would have failed in this one.
    /// Until the checks end, their results are held out of the cache and a save keeps no model
    /// ([`Model::saved_now`]), and the next `evaluate` waits for them before it starts.
    pub fn set_background_checks(&mut self, on: bool) {
        self.background_checks = on;
    }

    /// The SMLib checks the last evaluation left running have not been settled yet
    /// ([`Model::settle_feature_checks`]).
    pub fn checking_features(&self) -> bool {
        self.feature_checks.is_some()
    }

    /// Settle the SMLib checks the last evaluation left running ([`Model::set_background_checks`])
    /// once they have ended, or, with `wait`, after waiting for them. Their held results go into
    /// the cache as the checks before them pass. A check that refused a feature's bodies marks
    /// that feature and every result after it stale (keeping their geometry until the next
    /// `evaluate`), and the next `evaluate` fails the feature, on the same inputs, with the
    /// check's error. Returns whether an evaluation is owed to such a refusal: settled now or by
    /// an earlier call, and not evaluated since.
    pub fn settle_feature_checks(&mut self, wait: bool) -> bool {
        if let Some(pending) = self.feature_checks.clone()
            && let Some(outcome) = pending.outcome(wait, &self.cache, &self.kept)
        {
            self.feature_checks = None;
            if let Some(refused) = outcome {
                if let Some(at) = self.results.iter().position(|r| r.id == refused.id) {
                    for r in self.results.iter_mut().skip(at) {
                        r.stale = true;
                    }
                }
                // The fingerprints from the refused feature on are no longer the results'.
                self.fingerprinted = None;
                self.placement_inputs = None;
                self.carried_refusal = Some(refused);
                self.feature_refusal_owed = true;
            }
        }
        self.feature_refusal_owed
    }

    /// An import's SMLib check of its parts is still running ([`kernel::ImportReport::checks`]).
    pub fn checking_imports(&self) -> bool {
        self.results.iter().any(|r| r.import_report.as_ref().is_some_and(|i| i.checking()))
    }

    /// Fold what the ended checks of each import found into its result: its report's issues and
    /// its warning, as an import that waited for them has them. An import whose checks found a
    /// part to repair, that no one was told of yet, joins [`Self::take_import_notices`] (in
    /// history order). Checks still running are left to a later call.
    ///
    /// Which runs were told of is this model's own record ([`Self::import_notices`]), kept with
    /// its copies, so a copy evaluated beside it (a scratch or look-ahead evaluation) cannot take
    /// a notice from it, and a result restored from the evaluation cache is not told of twice.
    ///
    /// Checks that healed parts the result does not have yet ([`kernel::ImportChecks::heals`])
    /// make the import its healed bodies from then on: the heals join the import's fingerprint,
    /// and the import and every result after it are marked stale, keeping their geometry until
    /// the next [`Model::evaluate`] brings them up to date ([`Model::is_stale`]). Returns
    /// whether an evaluation is owed to such heals: settled now or by an earlier call, and not
    /// evaluated since (a caller that could not evaluate then is asked again).
    ///
    /// With the tolerance heal preference off ([`kernel::heals_tolerance_defects`]), a run that
    /// healed (begun while it was on) is not taken, nor what it found: the import is marked stale
    /// and evaluation owed, which reads it again as read.
    pub fn settle_import_checks(&mut self) -> bool {
        self.settle_import_checks_under(kernel::heals_tolerance_defects())
    }

    /// Call `wake` once the SMLib check of each import's parts still running ends
    /// ([`kernel::ImportChecks::on_end`]): a window asks for the frame the result needs when it
    /// lands, instead of asking [`Self::checking_imports`] every frame while the checks run.
    /// Every check that ends in the meantime calls it at once; a model with none running wakes
    /// nothing.
    pub fn on_import_checks_end(&self, wake: std::sync::Arc<dyn Fn() + Send + Sync>) {
        for r in &self.results {
            if let Some(report) = &r.import_report
                && report.checks.is_running()
            {
                report.checks.on_end(wake.clone());
            }
        }
    }

    /// The import `id` has a check still running, which the returned closure ends (tests): what
    /// [`kernel::ImportChecks::running_for_tests`] is for, on a model a window holds.
    #[doc(hidden)]
    pub fn running_import_check_for_tests(&mut self, id: u64) -> impl FnOnce(Vec<kernel::ImportIssue>) + Send + 'static {
        let (checks, end) = kernel::ImportChecks::running_for_tests();
        if let Some(r) = self.results.iter_mut().find(|r| r.id == id) {
            let report = r.import_report.as_deref().cloned().unwrap_or(kernel::ImportReport {
                parts: 0,
                solids: 0,
                issues: Vec::new(),
                single_root: true,
                split_edges: 0,
                checks: checks.clone(),
            });
            r.import_report = Some(std::sync::Arc::new(kernel::ImportReport { checks, ..report }));
        }
        end
    }

    /// [`Self::settle_import_checks`] with the heal preference given.
    pub(crate) fn settle_import_checks_under(&mut self, heal: bool) -> bool {
        let (found, told) = (&mut self.import_notices, &mut self.told_imports);
        let digests = &mut self.import_digests;
        let mut stale_from: Option<usize> = None;
        for (i, r) in self.results.iter_mut().enumerate() {
            let Some(report) = r.import_report.as_ref() else { continue };
            if report.checks.is_empty() || report.checking() {
                continue;
            }
            let run = &report.checks;
            // The read that brings healed parts in checks and heals the file again: that run ends
            // with the heals the model already has, and is not news.
            let heals = run.heals();
            let known = heals.as_ref().is_some_and(|h| digests.heals(r.id).is_some_and(|k| k.digest() == h.digest()));
            // With the heal preference off, a run that healed (begun while it was on) is not taken,
            // nor its notes (the heal's note over the part as read): the run is dropped, so no
            // view of the report shows them, and the import is read again.
            if heals.is_some() && !known && !heal {
                let as_read = kernel::ImportReport { checks: Default::default(), ..(**report).clone() };
                r.import_report = (!as_read.issues.is_empty()).then(|| Arc::new(as_read));
                stale_from = Some(stale_from.map_or(i, |s| s.min(i)));
                continue;
            }
            if let Some(heals) = heals
                && !known
                && let Some((_, text)) = digests.steps.get(&r.id)
            {
                digests.heals.insert(r.id, (*text, heals));
                stale_from = Some(stale_from.map_or(i, |s| s.min(i)));
            }
            let fresh = !known && run.issues().is_some_and(|i| !i.is_empty()) && !told.iter().any(|(id, t)| *id == r.id && t == run);
            if fresh {
                told.push((r.id, run.clone()));
            }
            let settled = report.current().into_owned();
            r.warning = import_warning(&settled.issues.iter().map(|i| i.detail.as_str()).collect::<Vec<_>>());
            r.import_report = (!settled.issues.is_empty()).then(|| Arc::new(settled));
            if fresh {
                found.push(r.id);
            }
        }
        if let Some(from) = stale_from {
            for r in self.results.iter_mut().skip(from) {
                r.stale = true;
            }
            // The fingerprints from the import on are no longer the results'.
            self.fingerprinted = None;
            self.placement_inputs = None;
            self.import_heals_owed = true;
        }
        self.import_heals_owed
    }

    /// Some import is its bodies as its checks healed them ([`Self::settle_import_checks`]).
    pub(crate) fn has_import_heals(&self) -> bool {
        !self.import_digests.heals.is_empty()
    }

    /// The imports whose checks found a part to repair since the last call
    /// ([`Self::settle_import_checks`]): a window says so for each, once.
    pub fn take_import_notices(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.import_notices)
    }

    /// The final model state.
    pub fn state(&self) -> Arc<ModelState> {
        let last = self.evaluated_state();
        match &self.held {
            Some(held) if Arc::ptr_eq(&held.base, &last) => held.shown.clone(),
            _ => last,
        }
    }

    /// The state after the last feature evaluated, without a failed sketch held for its editor.
    fn evaluated_state(&self) -> Arc<ModelState> {
        self.results.iter().rev().find(|r| !r.skipped).map(|r| r.state.clone()).unwrap_or_else(|| self.empty.clone())
    }

    /// Show the sketch `feature` while it is edited although it failed (#12085), as
    /// [`Model::failed_sketch_shown`] builds it, in the final state only: the sketch's result
    /// keeps its error and the features after it still fail. `None`, or a sketch that shows
    /// nothing, shows nothing more. Returns whether a held sketch is shown.
    pub fn hold_failed_sketch(&mut self, doc: &Document, feature: Option<u64>) -> bool {
        let Some(id) = feature else {
            self.held = None;
            return false;
        };
        let base = self.evaluated_state();
        if self.held.as_ref().is_some_and(|held| held.feature == id && Arc::ptr_eq(&held.base, &base)) {
            return true;
        }
        self.held = None;
        if base.sketch(id).is_some() {
            return false;
        }
        let Some(solved) = self.failed_sketch_shown(doc, id) else { return false };
        let mut shown = (*base).clone();
        shown.sketches.retain(|s| s.feature != id);
        shown.sketches.push(solved);
        self.held = Some(HeldSketch { feature: id, base, shown: Arc::new(shown) });
        true
    }

    /// The sketch `id` that failed because links lost their named face or edge (#12085): the
    /// evaluation keeps no sketch for it, so it would show nothing to repair. Built here as it is
    /// evaluated, on the model before it, but with each such link marked lost and kept where it
    /// was (no other face or edge is taken for it). `None` for a sketch that did not fail that
    /// way, or that fails for another reason too. For showing the sketch and telling that lost
    /// projections are its only failure (Redefine Sketch Plane); no feature builds from it.
    pub fn failed_sketch_shown(&self, doc: &Document, id: u64) -> Option<Arc<SolvedSketch>> {
        if !self.result(id).is_some_and(|r| r.lost_projections.is_some() && !r.skipped && !r.stale) {
            return None;
        }
        let f = doc.feature(id)?;
        let FeatureKind::Sketch { plane, sketch } = &f.kind else { return None };
        let mut next = (*self.state_before(id)).clone();
        let vals = evaluation_values(doc).0;
        let mut warning = None;
        let built = kernel::guard(&f.name, || {
            let _frames = ToolFrames::install(doc, f, &next);
            eval_sketch(doc, &vals, f, plane, sketch, &mut next, &mut warning, crate::project::LostNames::Mark)
                .map_err(|e| kernel::KernelError::Failed(e.to_string()))
        });
        built.ok().and_then(|()| next.sketches.iter().find(|s| s.feature == id).cloned())
    }

    /// State just before the feature with this id (for editing a sketch in context).
    pub fn state_before(&self, id: u64) -> Arc<ModelState> {
        let Some(i) = self.results.iter().position(|r| r.id == id) else { return self.state() };
        self.results.get(..i).and_then(|s| s.iter().rev().find(|r| !r.skipped)).map(|r| r.state.clone()).unwrap_or_else(|| self.empty.clone())
    }

    pub fn result(&self, id: u64) -> Option<&FeatureResult> {
        self.results.iter().find(|r| r.id == id)
    }

    /// Show a sketch's new solution before the model is evaluated again (the frames of an
    /// interactive drag): `sketch`, already solved with `report`, on the plane it was last
    /// evaluated on. The sketch's own result and the final state show it, and the sketch and
    /// every feature after it are marked stale, keeping their last geometry. Nothing else is
    /// recomputed; the next [`Model::evaluate`] recomputes from the sketch on. Returns `false`,
    /// changing nothing, when the sketch has no evaluated result or a feature before it is
    /// already stale (only a full evaluation brings that up to date).
    pub fn show_live_sketch(&mut self, feature: u64, sketch: solidcraft_sketch::Sketch, report: SolveReport) -> bool {
        let Some(i) = self.results.iter().position(|r| r.id == feature) else { return false };
        if self.results.iter().take(i).any(|r| r.stale) {
            return false;
        }
        let Some(last) = self.results.get(i).filter(|r| !r.skipped && r.error.is_none()).and_then(|r| r.state.sketch(feature)) else {
            return false;
        };
        let solved = Arc::new(SolvedSketch {
            feature,
            name: last.name.clone(),
            plane: last.plane,
            profiles: find_profiles(&sketch),
            profiles_3d: solidcraft_sketch::find_planar_3d_profiles(&sketch, last.plane),
            ends: solidcraft_sketch::loose_ends(&sketch),
            sketch,
            report,
        });
        let shown = self.results.iter().rposition(|r| !r.skipped);
        for (k, r) in self.results.iter_mut().enumerate().skip(i) {
            r.stale = true;
            // The sketch's own state and the final one show it: only that entry changes (the
            // states are copied on the first frame; later frames own them and swap in place).
            if (k == i || Some(k) == shown)
                && let Some(s) = Arc::make_mut(&mut r.state).sketches.iter_mut().find(|s| s.feature == feature)
            {
                *s = solved.clone();
            }
        }
        true
    }

    /// Some results are stale ([`Model::show_live_sketch`]): the model needs an `evaluate`.
    pub fn is_stale(&self) -> bool {
        self.results.iter().any(|r| r.stale)
    }

    /// Store in `doc` the sketches whose last evaluation solved them for new driving values
    /// (see `Document::solve_sketch`), so the stored sketch is the solved state the next value
    /// edit continues from (edits depend on their history, as a user editing dimensions one
    /// after another expects) and later evaluations repeat no continuation. Their results
    /// stand for the updated document: the solved sketch is what evaluating it gives.
    /// Returns how many sketches were stored.
    pub fn adopt_solved_sketches(&mut self, doc: &mut Document) -> usize {
        let solved: Vec<SolvedSketch> = self.edited_sketches(doc).cloned().collect();
        let Some(first) = solved.iter().filter_map(|s| doc.features.iter().position(|f| f.id == s.feature)).min() else { return 0 };
        self.placement_inputs = None;
        for s in &solved {
            if let Ok(stored) = doc.sketch_mut(s.feature) {
                adopt_solution(stored, &s.sketch);
            }
        }
        // The fingerprints from the first stored sketch on.
        let vals = evaluation_values(doc).0;
        let steps = ImportDigests::of(doc, &self.import_digests, self.heals());
        let marker = doc.marker.unwrap_or(usize::MAX);
        let mut prev_fp = first.checked_sub(1).and_then(|i| self.results.get(i)).map_or(0, |r| r.fingerprint);
        for (i, f) in doc.features.iter().enumerate().skip(first) {
            let before = self.state_before(f.id);
            prev_fp = chain_fingerprint(doc, &vals, f, prev_fp, i >= marker, &steps, Some(&before));
            if let Some(r) = self.results.get_mut(i).filter(|r| r.id == f.id) {
                r.fingerprint = prev_fp;
            }
        }
        solved.len()
    }

    /// The sketches the last evaluation solved for driving values other than the ones `doc`
    /// stores them with (a value edit since they were stored), or with their linked points
    /// elsewhere (a value edit upstream moved what they project: stored, the next evaluation
    /// starts where this one ended instead of walking the links there again).
    pub fn edited_sketches<'a>(&'a self, doc: &'a Document) -> impl Iterator<Item = &'a SolvedSketch> + 'a {
        self.results.iter().filter(|r| !r.skipped && r.error.is_none()).filter_map(|r| r.state.sketch(r.id)).filter(|s| {
            s.report.keeps_orientation()
                && doc.sketch(s.feature).is_ok_and(|stored| {
                    stored.driving_values() != s.sketch.driving_values()
                        || stored.patterns != s.sketch.patterns
                        // 3D dimensions and 3D points placed by expressions (#10608).
                        || stored.wire_constraints != s.sketch.wire_constraints
                        || stored.wires.iter().zip(&s.sketch.wires).any(|(a, b)| a.link.is_none() && a.id == b.id && a.fit != b.fit)
                        || s.sketch.texts_follow_other_values(stored)
                        || s.sketch.points.iter().zip(&stored.points).any(|(p, q)| p.link.is_some() && p.id == q.id && p.pos != q.pos)
                })
        })
    }

    /// While `kept` is set, every result this model evaluates or takes from the cache is also
    /// kept there, and a result the cache no longer has is taken from there (#1989): an
    /// evaluation ahead on a copy keeps what it computes, and the run that follows on the model
    /// itself finds it all, whatever the cache dropped meanwhile. Copies of the model keep
    /// nothing.
    pub fn keep(&mut self, kept: Option<Arc<KeptResults>>) {
        self.kept = KeptSlot(kept);
    }

    /// Where this model keeps its results besides the cache, if it does ([`Model::keep`]).
    pub fn kept(&self) -> Option<Arc<KeptResults>> {
        self.kept.0.clone()
    }

    /// Whether the results are `doc`'s evaluation: one per feature, each on the inputs `doc`
    /// gives it, none stale, so [`Model::evaluate`] would keep every one of them. A document
    /// changed without an evaluation since is not.
    pub fn is_current(&self, doc: &Document) -> bool {
        if self.results.len() != doc.features.len() || self.is_stale() {
            return false;
        }
        let vals = evaluation_values(doc).0;
        // as the next evaluation would fingerprint them: under the preference as it is now
        let steps = ImportDigests::of(doc, &self.import_digests, kernel::heals_tolerance_defects());
        let marker = doc.marker.unwrap_or(usize::MAX);
        let mut prev = 0;
        // The model each feature starts from (which components the bodies it names are in).
        let mut state = self.empty.clone();
        doc.features.iter().zip(&self.results).enumerate().all(|(i, (f, r))| {
            prev = chain_fingerprint(doc, &vals, f, prev, i >= marker, &steps, Some(&state));
            let posed = !pose_only(f) || self.poses.get(&f.id) == Some(&placement_digest(doc, f, Some(&state)));
            if !r.skipped {
                state = r.state.clone();
            }
            r.id == f.id && r.fingerprint == prev && posed
        })
    }

    /// Take the results `ahead` (a copy of this model, evaluated off this thread for the same
    /// commands) computed: until [`Model::unprime`], `evaluate` reuses each one whose feature
    /// has the same inputs here, instead of evaluating it again. Results that don't match are
    /// ignored, so a design changed meanwhile is evaluated as usual.
    pub fn prime(&mut self, ahead: &Model) {
        self.primed.clear();
        self.prime_also(ahead);
    }

    /// Offer the results `ahead` computed as well as those offered already ([`Model::prime`]):
    /// a feature takes the first one offered whose inputs are its own here. A pose-only
    /// feature's result is taken only where `ahead` evaluated it at the pose it has here: its
    /// fingerprint leaves the placement out, and `ahead` may be a model built before the bodies
    /// it places moved (another configuration row's).
    pub fn prime_also(&mut self, ahead: &Model) {
        let results = ahead.results.iter().filter(|r| !r.skipped && !r.stale).map(|r| (r.id, r.clone())).collect();
        self.primed.push(Primed { context: ahead.context, results: Arc::new(results), poses: Arc::new(ahead.poses.clone()) });
    }

    /// How many models' results are offered ([`Model::prime_also`]).
    pub fn primed_count(&self) -> usize {
        self.primed.len()
    }

    /// Stop offering the results offered after the first `count` ([`Model::primed_count`]),
    /// keeping those an earlier caller offered.
    pub fn unprime_after(&mut self, count: usize) {
        self.primed.truncate(count);
    }

    /// Stop offering the results [`Model::prime`] took.
    pub fn unprime(&mut self) {
        self.primed.clear();
    }

    /// Offer the import reader's validated bodies for the new feature's first evaluation.
    pub fn prime_mesh_import(&mut self, id: u64, meshes: PreparedMeshImport) {
        self.prepared_meshes = Some((id, meshes));
        self.placement_inputs = None;
    }

    /// When SMLib's check runs on the features this model evaluates.
    pub fn validation(&self) -> kernel::Validation {
        self.validation
    }

    /// Whether this model's results were evaluated with the tolerance heal preference on
    /// ([`kernel::heals_tolerance_defects`]): an import's failing parts healed, Repair healing
    /// first. A model evaluated with it off holds other results: a saved one is not taken up
    /// with the preference on, nor the reverse.
    pub fn heals(&self) -> bool {
        !self.heal_off
    }

    /// Check the features evaluated from now on under `validation` ([`kernel::Validation`]):
    /// after every sweep, boolean and blend, once on the bodies each feature keeps, or not at
    /// all. The results already there stand until their features are evaluated again; the cache
    /// keeps results apart by mode, so one evaluated under another mode is not taken up.
    pub fn set_validation(&mut self, validation: kernel::Validation) {
        self.validation = validation;
    }

    /// Re-evaluate the document, reusing results up to the first changed feature.
    pub fn evaluate(&mut self, doc: &Document) {
        // The checks the last evaluation left running stand before anything is built on them.
        self.settle_feature_checks(true);
        self.feature_refusal_owed = false;
        let carried = self.carried_refusal.take();
        let prepared_meshes = self.prepared_meshes.take();
        // A failed sketch held for its editor is built again from the evaluated design: when the
        // sketch and every feature after it fail, the results end in the state they ended in
        // before, which would keep showing the sketch as it was before the edit.
        self.held = None;
        self.import_heals_owed = false;
        let _span = solidcraft_profile::span("doc", "evaluate");
        self.note_states();
        self.placement_inputs = None;
        self.fingerprinted = None;
        self.placement_local = true;
        let (vals, perr) = evaluation_values(doc);
        self.param_errors = perr;
        self.frame_bodies.lock().unwrap_or_else(|e| e.into_inner()).next_generation();
        let mut prev_fp = 0u64;
        // The names the design must not use again travel with the state (#4957).
        let mut state = std::sync::Arc::new(ModelState { reserved_body_names: doc.reserved_body_names.clone(), ..(*self.empty).clone() });
        // Those and the names stored on the suppressed features so far (#4302).
        let mut reserved = doc.reserved_body_names.clone();
        let mut out: Vec<FeatureResult> = Vec::with_capacity(doc.features.len());
        let mut reuse = !self.no_cache;
        let mut recomputed = 0;
        let mut restored = 0;
        let marker = doc.marker.unwrap_or(usize::MAX);
        let validation = self.validation;
        // read once: the context, the fingerprints and the model's record agree
        let heal = kernel::heals_tolerance_defects();
        self.heal_off = !heal;
        // The features' own inputs, without where they are placed: a part that moves leaves them
        // as they were. An import keeps its result while they are the same, so a move of a part
        // does not read the file again (#17310). Only a design with an import needs the digest.
        let with_import = doc.features.iter().any(|f| matches!(f.kind, FeatureKind::Import { .. }));
        let mut geo = 0u64;
        let ctx = context_digest(doc, &vals) ^ validation_digest(validation) ^ heal_digest(heal);
        let shared_ctx = shared_context_digest(doc, &vals) ^ validation_digest(validation) ^ heal_digest(heal);
        let previous_context = self.context;
        let mut rolled_results = std::mem::take(&mut self.rolled_results);
        self.context = ctx;
        self.import_digests = ImportDigests::of(doc, &self.import_digests, heal);
        let primed: Vec<Primed> = self.primed.iter().filter(|p| !self.no_cache && p.context == ctx).cloned().collect();
        // Each feature's component, for `Document::legacy_body_component` on every body after
        // every feature without searching the timeline for each (the first feature with an id).
        let mut feature_component: std::collections::HashMap<u64, u64> = std::collections::HashMap::with_capacity(doc.features.len());
        for f in &doc.features {
            feature_component.entry(f.id).or_insert(f.component);
        }
        // The component envelopes' features: the bodies they made are marked after every feature.
        let envelopes: std::collections::HashSet<u64> =
            doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::BoundingSolid { contents: true, .. })).map(|f| f.id).collect();
        let watched = crate::progress::begin(doc.features.len());
        let mut poses = std::collections::HashMap::new();
        // Pose-only features evaluated again for a pose change alone: their new bodies, which
        // replace the old ones in the results reused after them (#12139).
        let mut moved: Vec<(u64, Vec<ModelBody>)> = Vec::new();
        // The features that failed so far in this pass, by id: their names and errors (#9075).
        let mut failed: std::collections::HashMap<u64, (String, String)> = std::collections::HashMap::new();
        // SMLib's check of a feature's bodies runs on another thread while the features after it
        // are evaluated ([`overlap`]). Their results, and the cache entries held until every
        // check before them passed, stand only once those checks pass; a refusal takes the pass
        // back to the refused feature, which then fails with the check's error, as it would have.
        let mut checks = overlap::Checks::new(validation);
        let mut start = 0;
        let mut back: Option<overlap::Refused> = None;
        'pass: loop {
            if let Some(refused) = back.take() {
                let s = refused.saved;
                start = refused.index;
                state = s.state;
                reserved = s.reserved;
                out.truncate(s.out_len);
                reuse = s.reuse;
                prev_fp = s.prev_fp;
                geo = s.geo;
                recomputed = s.recomputed;
                restored = s.restored;
                poses = s.poses;
                moved = s.moved;
                failed = s.failed;
                rolled_results = s.rolled_results;
                checks.refuse(refused.index, refused.failure);
            }
            for (i, f) in doc.features.iter().enumerate().skip(start) {
                if let Some(refused) = checks.settle(false, &self.cache, &self.kept) {
                    back = Some(refused);
                    continue 'pass;
                }
                let (reuse_in, prev_fp_in) = (reuse, prev_fp);
                // The digest of the features before this one, for an import's key (#17310).
                let geo_in = geo;
                if let Some(r) = out.last()
                    && let Some(e) = &r.error
                {
                    failed.insert(r.id, (r.name.clone(), e.clone()));
                }
                self.placement_local = self.placement_local && placement_reuse::local_geometry(f, &feature_component);
                let rolled_back = i >= marker;
                // Retain at most one active result per rolled-back feature, even across several
                // marker moves. Build the map anew so deleted features release their states too.
                let rolled = rolled_results.remove(&f.id);
                let rolled = if self.no_cache {
                    None
                } else if rolled_back {
                    let active = self.results.get(i).filter(|r| r.id == f.id && !r.skipped && !r.stale && !r.unkept);
                    if let Some(saved) = active.map(|r| (previous_context, r.clone())).or(rolled) {
                        self.rolled_results.insert(f.id, saved);
                    }
                    None
                } else {
                    rolled.filter(|(context, _)| *context == ctx).map(|(_, r)| r)
                };
                // Opening was cancelled: the rest is skipped and the result discarded.
                let cancelled = watched.step(i);
                let fp = chain_fingerprint(doc, &vals, f, prev_fp, rolled_back, &self.import_digests, Some(&state));
                prev_fp = fp;
                if with_import {
                    // This feature's own inputs, without where it is placed: the digest of the
                    // features before an import says whether the model around it changed (#17310).
                    geo ^= fingerprint(doc, 0, f, &vals, rolled_back, &self.import_digests);
                }
                let pose = pose_only(f).then(|| placement_digest(doc, f, Some(&state)));
                if let Some(pose) = pose {
                    poses.insert(f.id, pose);
                }
                let posed = pose.is_none_or(|pose| self.poses.get(&f.id) == Some(&pose));
                let current = |old: &&FeatureResult| old.id == f.id && old.fingerprint == fp && !old.stale && posed;
                // A state the saved model did not keep is evaluated again from the one before it; the
                // results after it that the model did keep still stand (#3220).
                let unkept = reuse && self.results.get(i).filter(current).is_some_and(|old| old.unkept);
                if f.suppressed && !rolled_back {
                    reserved.extend(f.body_names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).map(str::to_string));
                }
                // A feature that names its pieces as later features pick them (saved before format 14)
                // has a result only while those picks are what they were.
                if reuse
                    && !unkept
                    && let Some(old) = self
                        .results
                        .get(i)
                        .filter(current)
                        .filter(|old| moved.is_empty() || old.skipped || reads_none_of(f, &state, &moved, &feature_component))
                        .filter(|old| old.skipped || legacy_current(doc, f, &state, &old.state))
                {
                    if moved.is_empty() {
                        if !old.skipped {
                            state = old.state.clone();
                        }
                        out.push(old.clone());
                    } else {
                        if !old.skipped {
                            state = with_moved(&old.state, &moved);
                        }
                        out.push(FeatureResult { state: state.clone(), ..old.clone() });
                    }
                    continue;
                }
                // Reuse goes on past a pose-only feature evaluated again for its pose alone.
                let reusing = reuse && !unkept && pose.is_some();
                reuse = unkept && !cancelled;
                if f.suppressed || rolled_back || cancelled {
                    out.push(FeatureResult {
                        id: f.id,
                        name: f.name.clone(),
                        error: None,
                        error_code: None,
                        internal: false,
                        warning: None,
                        import_report: None,
                        repair: None,
                        lost_projections: None,
                        ms: 0.0,
                        skipped: true,
                        stale: false,
                        fingerprint: fp,
                        unkept: false,
                        state: state.clone(),
                    });
                    continue;
                }
                // Taken from the design each time: a state taken from an earlier evaluation may have
                // been made before a rename freed a name or a feature was suppressed.
                if state.reserved_body_names != reserved {
                    Arc::make_mut(&mut state).reserved_body_names.clone_from(&reserved);
                }
                // A sketch the feature is built from failed earlier in this pass: the feature fails naming
                // that sketch, with its error, as one using a failed parameter does (#9075), not as a
                // sketch that must come earlier in the timeline. Never cached: the cache key holds
                // neither the sketch's name nor its error (a rename, another failing value).
                let failed_sketch = if failed.is_empty() {
                    None
                } else {
                    f.kind.input_sketches().into_iter().find_map(|id| failed.get(&id)).map(|(name, e)| format!("sketch `{name}` failed: {e}"))
                };
                if let Some(e) = failed_sketch {
                    recomputed += 1;
                    reuse = false;
                    out.push(FeatureResult {
                        id: f.id,
                        name: f.name.clone(),
                        error: Some(e),
                        error_code: None,
                        internal: false,
                        warning: None,
                        import_report: None,
                        repair: None,
                        lost_projections: None,
                        ms: 0.0,
                        skipped: false,
                        stale: false,
                        fingerprint: fp,
                        unkept: false,
                        state: state.clone(),
                    });
                    continue;
                }
                // The same feature on the same inputs: its earlier result.
                let key = EvalKey::new(doc, &vals, f, fp, ctx, shared_ctx, &state, &self.import_digests, geo_in);
                // A check that refused this feature's bodies on these inputs after its evaluation
                // returned: it fails as it would have failed there.
                if let Some(c) = carried.as_ref().filter(|c| c.id == f.id && c.key == key.whole) {
                    checks.refuse(i, c.failure.clone());
                }
                let kept = self.kept.0.as_ref().filter(|_| !self.no_cache);
                if !self.no_cache
                    && let Some(c) = self
                        .cache
                        .lock()
                        .ok()
                        .and_then(|mut m| m.state(key.whole))
                        .or_else(|| kept.and_then(|k| k.get(key.whole)))
                        .filter(|c| c.error.is_some() || legacy_current(doc, f, &state, &c.state))
                {
                    if let Some(k) = kept {
                        k.put(key.whole, &c);
                    }
                    restored += 1;
                    if c.error.is_none() {
                        state = c.state.clone();
                    }
                    // A kept result after a failed one would not be this evaluation's.
                    reuse &= c.error.is_none();
                    if reusing
                        && c.error.is_none()
                        && let Some(bodies) = pose_moved(self.results.get(i), f, fp, &state)
                    {
                        moved.retain(|(id, _)| *id != f.id);
                        moved.push((f.id, bodies));
                        reuse = true;
                    }
                    // Not kept in the cached result: its notes follow the sketches as they are now.
                    let warning = if c.error.is_none() { with_retained_notes(doc, c.warning, f, &state) } else { c.warning };
                    out.push(FeatureResult {
                        id: f.id,
                        name: f.name.clone(),
                        error: c.error,
                        error_code: c.error_code,
                        internal: false,
                        warning,
                        import_report: c.import_report,
                        repair: c.repair,
                        lost_projections: c.lost_projections,
                        ms: c.ms,
                        skipped: false,
                        stale: false,
                        fingerprint: fp,
                        unkept: false,
                        state: state.clone(),
                    });
                    continue;
                }
                // An active result hidden by the marker, or evaluated ahead on these same inputs
                // (the same chain of definitions from the start). Changed inputs never reuse it.
                if let Some(r) = rolled
                    .as_ref()
                    .filter(|_| moved.is_empty())
                    .filter(current)
                    .or_else(|| {
                        primed.iter().filter(|_| moved.is_empty()).find_map(|p| {
                            // posed where that model evaluated it, not only here
                            let posed_there = pose.is_none_or(|pose| p.poses.get(&f.id) == Some(&pose));
                            p.results.get(&f.id).filter(current).filter(|_| posed_there)
                        })
                    })
                    .filter(|r| r.error.is_some() || legacy_current(doc, f, &state, &r.state))
                {
                    restored += 1;
                    if r.error.is_none() {
                        state = r.state.clone();
                    }
                    reuse &= r.error.is_none();
                    let warning = if r.error.is_none() { with_retained_notes(doc, r.warning.clone(), f, &state) } else { r.warning.clone() };
                    out.push(FeatureResult { state: state.clone(), unkept: false, warning, ..r.clone() });
                    continue;
                }
                // Or what it changed when what it reads was as it is now, made again on the rest of the
                // model as it is now. Never in place of a result a check refused.
                let reads = key.reads.as_ref().filter(|_| {
                    !self.no_cache && checks.refused_at(i).is_none() && param_names(f).iter().all(|n| !self.param_errors.contains_key(n))
                });
                let relative = reads.and(key.relative);
                let change_at = |k: u64| self.cache.lock().ok().and_then(|mut m| m.change(k));
                let by_relative = relative.and_then(change_at).and_then(|c| state.with_relative(&c.state, f.id).map(|next| (next, c)));
                let hit_relative = by_relative.is_some();
                let reused = by_relative
                    .or_else(|| reads.and_then(|(k, _)| change_at(*k)).and_then(|c| state.with_change(&c.state, f.id).map(|next| (next, c))));
                // Each feature computed is a profiler span named after its kind.
                let _feature_span = solidcraft_profile::span("feature", f.kind.type_name());
                let before = state.clone();
                // A kernel call that failed before this feature is not its failure.
                kernel::failure::clear();
                let t0 = now();
                let (mut next, cached) = match reused {
                    Some((next, c)) => {
                        restored += 1;
                        (next, Some(c))
                    }
                    None => {
                        recomputed += 1;
                        let mut next = (*state).clone();
                        // The names the design must not use again travel with the state (#4957, #4302).
                        next.reserved_body_names.clone_from(&reserved);
                        next.frame_bodies = Arc::downgrade(&self.frame_bodies);
                        for b in &mut next.bodies {
                            b.made = crate::lineage::Made::Before;
                        }
                        (next, None)
                    }
                };
                let mut warning = cached.as_ref().and_then(|c| c.warning.clone());
                let mut import_report = cached.as_ref().and_then(|c| c.import_report.clone());
                let mut repair = cached.as_ref().and_then(|c| c.repair.clone());
                let mut lost_projections = cached.as_ref().and_then(|c| c.lost_projections.clone());
                // Face references by name: move each face point onto its named face (on the body the
                // feature names, when it names one and the face is there).
                let renamed;
                let mut frame_error: Option<String> = None;
                let f = match with_named_face_points(doc, &vals, &state, f) {
                    Ok(Some((g, notes))) => {
                        if !notes.is_empty() {
                            warning = Some(notes.join("; "));
                        }
                        next.named_faces.insert(f.id, g.kind.face_points());
                        renamed = g;
                        &renamed
                    }
                    Ok(None) => f,
                    Err(e) => {
                        frame_error = Some(e.in_feature(&f.name).to_string());
                        f
                    }
                };
                // A parameter with an error fails the features using it, with that error.
                let bad_param = param_names(f).into_iter().find_map(|n| self.param_errors.get(&n).map(|e| format!("parameter `{n}`: {e}")));
                // The bodies whose check runs beside the features after this one.
                let mut apart: Option<(Vec<(String, kernel::KeptCheck)>, Vec<(String, Body, bool)>)> = None;
                let refused = checks.refused(i);
                // The feature failed in the kernel (not for a parameter, a reference or a rule of
                // the design's): its kernel call's code is the last that failed on this thread.
                let kernel_failed = std::cell::Cell::new(false);
                let mut r = match frame_error.or(bad_param) {
                    _ if cached.is_some() => Ok(()),
                    Some(e) => Err(kernel::KernelError::Failed(e)),
                    None if refused.is_some() => {
                        let refused = refused.clone().map(|x| (x.error, x.warning, x.import_report, x.repair));
                        match refused {
                            Some((e, w, ir, rp)) => {
                                (warning, import_report, repair) = (w, ir, rp);
                                Err(e)
                            }
                            None => Ok(()),
                        }
                    }
                    None => kernel::guard(&f.name, || {
                        let _val_guard = EvalValidationGuard::new(validation);
                        let (r, deferred) = kernel::validating(validation, || {
                            IMPORT_HEALS.with(|h| *h.borrow_mut() = self.import_digests.heals(f.id).cloned());
                            let meshes = prepared_meshes.as_ref().filter(|(id, _)| *id == f.id).map(|(_, meshes)| meshes);
                            let r = eval_feature(doc, &vals, f, &mut next, &mut warning, &mut import_report, &mut repair, heal, meshes);
                            IMPORT_HEALS.with(|h| *h.borrow_mut() = None);
                            r
                        });
                        r.map_err(|e| match e {
                            // A panic the kernel caught stays one.
                            crate::DocError::Kernel(kernel::KernelError::Internal(m)) => kernel::KernelError::Internal(m),
                            e => {
                                kernel_failed.set(matches!(e, crate::DocError::Kernel(_)));
                                if let crate::DocError::LostProjections { links, .. } = &e {
                                    lost_projections = Some(Arc::new(links.clone()));
                                }
                                kernel::KernelError::Failed(e.in_feature(&f.name).to_string())
                            }
                        })?;
                        // The checks the feature's steps left to its end: the bodies it keeps.
                        if !deferred {
                            Ok(())
                        } else if let Some(detached) = checks.detach(&state, &next) {
                            apart = Some((detached, replaced(&state, &next)));
                            Ok(())
                        } else {
                            check_kept(&state, &next)
                        }
                    }),
                };
                if r.is_ok()
                    && let Some(after) = apart.take()
                {
                    // Where a refusal of this feature's check, running beside the features after
                    // it, takes the evaluation back to: the loop's state before this feature.
                    let saved = overlap::Saved {
                        state: state.clone(),
                        reserved: reserved.clone(),
                        out_len: out.len(),
                        reuse: reuse_in,
                        prev_fp: prev_fp_in,
                        geo: geo_in,
                        recomputed: recomputed - 1,
                        restored,
                        poses: poses.clone(),
                        moved: moved.clone(),
                        failed: failed.clone(),
                        rolled_results: rolled_results.clone(),
                    };
                    let failure = overlap::Failure {
                        error: kernel::KernelError::Failed(String::new()),
                        warning: warning.clone(),
                        import_report: import_report.clone(),
                        repair: repair.clone(),
                        ms: t0.elapsed().as_secs_f64() * 1000.0,
                    };
                    let name = f.name.clone();
                    let (after, inputs) = after;
                    r = checks.start(i, f.id, key.whole, saved, failure, move || kernel::guard(&name, || run_kept(after, inputs)));
                }
                if r.is_ok() && cached.is_none() {
                    // Profiles picked by curves that no longer bound a profile exactly were found
                    // by their best match: say which.
                    let thin = matches!(&f.kind, FeatureKind::Extrude { thin: Some(_), .. });
                    for (sketch, sel) in f.kind.profile_sels() {
                        if let (false, Some(ss)) = (thin, state.sketch(sketch)) {
                            for note in crate::profile_fallback_notes(ss, sel) {
                                append_warning(&mut warning, note);
                            }
                        }
                    }
                    track_altered_sheets(&state, &mut next, f, &mut warning);
                    note_join_apart(doc, &state, &next, f, &mut warning);
                    note_cut_cavity(&state, &next, f, &mut warning);
                }
                // What it cut from a sheet made from surfaces goes into the sheet's records (see
                // `absorb_sheet_cuts`); a cached result already holds them.
                if cached.is_none() {
                    r = r.and_then(|()| {
                        absorb_sheet_cuts(doc, &state, &mut next, f, validation, &mut warning).map_err(|e| kernel::KernelError::Failed(e.to_string()))
                    });
                }
                let ms = match (&cached, refused) {
                    (Some(c), _) => c.ms,
                    (None, Some(x)) => x.ms,
                    (None, None) => t0.elapsed().as_secs_f64() * 1000.0,
                };
                let internal = matches!(r, Err(kernel::KernelError::Internal(_)));
                // What it changed, when that is only what it read: kept for whatever the rest of the
                // model holds, and its bodies are known by what it read.
                let change = match (&cached, reads) {
                    (None, Some((_, read))) if r.is_ok() => next.change_from(&state, f, read),
                    _ => None,
                };
                // A change that holds whatever the bodies it did not read is known by the key
                // without them, and so are the bodies it made.
                let relative = relative.filter(|_| hit_relative || change.as_ref().is_some_and(|c| c.relative.is_some()));
                let stamp_key = match (reads, relative) {
                    (Some(_), Some(k)) => k,
                    (Some((k, _)), None) if cached.is_some() || change.is_some() => *k,
                    _ => key.whole,
                };
                let error = match r {
                    Ok(()) => {
                        // Copies are named after their sources through the instance transforms.
                        let mats = own_copies(doc, &vals, &state, f).unwrap_or_default();
                        // Copies made through a pattern or mirror among the sources come after (#395).
                        // A Pattern of components places occurrences in reconcile, not here (#12116).
                        let mats = match &f.kind {
                            FeatureKind::Pattern { features, bodies, components, .. } if bodies.is_empty() && components.is_empty() => {
                                copy_instances(doc, &vals, &state, f, features, &mats).unwrap_or(mats)
                            }
                            FeatureKind::Mirror { features, bodies, .. } if bodies.is_empty() => {
                                copy_instances(doc, &vals, &state, f, features, &mats).unwrap_or(mats)
                            }
                            _ => mats,
                        };
                        // A revolve axis origin that follows parameters names its faces from its values.
                        let named = naming_feature(&vals, f);
                        next.stamp(doc, &state, &named, stamp_key, mats);
                        // Evaluated again only for what came before it (a mirror moved by a drag).
                        if let Some(old) = self.results.get(i).filter(|old| old.id == f.id) {
                            let made = old.state.bodies.iter().filter(|b| b.feature == f.id).map(|b| (b.name.as_str(), b));
                            let made: std::collections::HashMap<&str, &ModelBody> = made.collect();
                            for b in next.bodies.iter_mut().filter(|b| b.feature == f.id) {
                                if let Some(old) = made.get(b.name.as_str()) {
                                    b.keep_meshes_of(old);
                                }
                            }
                        }
                        for b in &mut next.bodies {
                            b.envelope = envelopes.contains(&b.feature);
                            if !b.transferred {
                                b.component = match doc.body_components.get(&b.name) {
                                    Some(c) => *c,
                                    None => feature_component.get(&b.feature).copied().unwrap_or(0),
                                };
                            }
                        }
                        state = Arc::new(next);
                        // A window opening the design can show what is built so far.
                        watched.offer(i + 1, doc, &state);
                        None
                    }
                    Err(e) => Some(e.to_string().trim_start_matches("the operation failed: ").to_string()),
                };
                // The kernel call it failed with, when it failed in the kernel; a result taken from the
                // cache keeps the code it was made with.
                let error_code = match (&cached, &error) {
                    (Some(c), Some(_)) => c.error_code.clone(),
                    (None, Some(_)) if kernel_failed.get() => kernel::failure::last(),
                    (None, Some(_)) => None,
                    (_, None) => None,
                };
                let lost_projections = lost_projections.filter(|_| error.is_some());
                // A panic is no result to reuse: the kernel was replaced after it. Work its watcher
                // stopped may have been cut short (a boolean that gave up): it is not kept either.
                if !self.no_cache && !watched.cancelled() && !internal {
                    let whole = CachedEval {
                        error: error.clone(),
                        error_code: error_code.clone(),
                        warning: warning.clone(),
                        import_report: import_report.clone(),
                        repair: repair.clone(),
                        lost_projections: lost_projections.clone(),
                        ms,
                        state: state.clone(),
                    };
                    let change = reads.zip(change).map(|((k, _), change)| (relative.unwrap_or(*k), whole.with(change)));
                    let entry = CacheEntry { key: key.whole, bytes: state.added_bytes(&before), whole, change };
                    // Held while a check before it, or its own, runs.
                    checks.store(i, entry, &self.cache, &self.kept);
                }
                // Not in the cached result: its notes follow the sketches as they are now.
                let warning = if error.is_none() { with_retained_notes(doc, warning, f, &state) } else { warning };
                reuse &= error.is_none();
                if reusing
                    && error.is_none()
                    && let Some(bodies) = pose_moved(self.results.get(i), f, fp, &state)
                {
                    moved.retain(|(id, _)| *id != f.id);
                    moved.push((f.id, bodies));
                    reuse = true;
                }
                out.push(FeatureResult {
                    id: f.id,
                    name: f.name.clone(),
                    error,
                    error_code,
                    internal,
                    warning,
                    import_report,
                    repair,
                    lost_projections,
                    ms,
                    skipped: false,
                    stale: false,
                    fingerprint: fp,
                    unkept: false,
                    state: state.clone(),
                });
            }
            // Every check still running ends before the results stand, unless they are left to end
            // beside the caller ([`Model::set_background_checks`]): those that ended are settled.
            if let Some(refused) = checks.settle(!self.background_checks, &self.cache, &self.kept) {
                back = Some(refused);
                continue 'pass;
            }
            // Main's evaluate loop ends the pass here; Pattern-of-components reconcile runs from Session::refresh.
            break;
        }
        self.feature_checks = checks.running().then(|| overlap::Pending::new(checks));
        self.placement_local = self.placement_local && out.iter().all(|r| !r.skipped && !r.stale && !r.unkept && r.error.is_none());
        self.poses = poses;
        self.results = out;
        self.note_states();
        // A cached import's result is as its check left it, not as the import first showed.
        self.settle_import_checks();
        self.last_recomputed = recomputed;
        self.recomputed += recomputed;
        self.last_restored = restored;
    }
}

impl Model {
    /// Follow a rename of body `old` to `new` without evaluating anything (#3044): `doc` is the
    /// design after the rename, which changed nothing but names, so every result keeps its
    /// geometry and meshes and only the body's name (and the names worked out from it) changes.
    /// Later features that change the body (a pattern, a chamfer, a join) keep their results
    /// too (#2467): a feature's geometry doesn't depend on its bodies' names. A body no later
    /// feature changes is followed even when other bodies are numbered after it (`X (2)`, the
    /// solids of a STEP file that share a label): `body.rename` keeps their names.
    /// False, leaving the model as it was, when the rename can't be followed that way and the
    /// design must be evaluated: the model isn't the design's, a body of that name comes and goes
    /// or isn't the same body throughout, a later body's name is worked out from a body later
    /// features change, a message quotes it, or it is sheet metal, threaded, or named in a later
    /// feature's face references.
    pub fn rename_body(&mut self, doc: &Document, old: &str, new: &str) -> bool {
        if self.results.len() != doc.features.len() || self.results.iter().zip(&doc.features).any(|(r, f)| r.id != f.id || r.stale) {
            return false;
        }
        let Some(at) = self.results.iter().position(|r| r.state.body(old).is_some()) else { return false };
        let Some((feature, made)) = self.results.get(at).and_then(|r| r.state.body(old)).map(|b| (b.feature, Arc::as_ptr(&b.mesh))) else {
            return false;
        };
        // Untouched from the feature that made it to the end: `body.rename` keeps the names of
        // its siblings and of later bodies named after it (`X (2)`, the solids of a STEP file
        // with one label), so evaluating the renamed design gives the same names.
        let untouched = self.results.iter().skip(at).all(|r| r.state.body(old).is_some_and(|b| Arc::as_ptr(&b.mesh) == made));
        // Or one body from the feature that made it until it is gone (joined into another), and
        // no other body's name comes from its name, unless the feature that made that body
        // stores the name (`body.rename` pins a split's other piece, `X (1)`, #12657): the
        // renamed design builds it under that name too.
        let held: Vec<bool> = self.results.iter().skip(at).map(|r| r.state.body(old).is_some()).collect();
        let one_run = held.iter().skip_while(|h| **h).all(|h| !*h);
        let derived = format!("{old} (");
        let stored = |b: &ModelBody| doc.feature(b.feature).is_some_and(|f| f.body_names.contains(&b.name));
        let same_body = || {
            self.results
                .iter()
                .skip(at)
                .all(|r| r.state.bodies.iter().all(|b| if b.name == old { b.feature == feature } else { !b.name.starts_with(&derived) || stored(b) }))
        };
        let quoted = self.results.iter().skip(at).any(|r| [&r.error, &r.warning].into_iter().flatten().any(|m| m.contains(old)));
        let named_elsewhere = self.results.iter().any(|r| {
            r.state.body(new).is_some() || r.state.surface_sheets.iter().any(|x| x.body == old) || r.state.threads.iter().any(|t| t.body == old)
        });
        let faces_named = doc.features.iter().skip(at + 1).any(|f| f.face_names.iter().any(|n| n.contains(old)));
        if !(untouched || (one_run && same_body())) || quoted || named_elsewhere || faces_named {
            return false;
        }
        // States shared between results stay shared.
        let mut renamed: std::collections::HashMap<*const ModelState, Arc<ModelState>> = std::collections::HashMap::new();
        for r in self.results.iter_mut().skip(at) {
            let state = renamed
                .entry(Arc::as_ptr(&r.state))
                .or_insert_with(|| {
                    let mut st = (*r.state).clone();
                    for b in st.bodies.iter_mut().filter(|b| b.name == old) {
                        b.name = new.to_string();
                        b.names = b.names.renamed(old, new);
                    }
                    // The names the design must not use again travel with the state (#4957).
                    st.reserved_body_names = doc.reserved_body_names.clone();
                    Arc::new(st)
                })
                .clone();
            r.state = state;
        }
        self.refingerprint(doc);
        true
    }

    /// Follow a rename of feature `id` to `doc`'s name for it without evaluating anything: a
    /// feature's name labels it (its result, its sketch, its construction geometry) but builds
    /// nothing. False, leaving the model as it was, when the model isn't the design's, the
    /// feature failed, or a later failed feature's error quotes its old name: evaluate then.
    pub fn rename_feature(&mut self, doc: &Document, id: u64) -> bool {
        if self.results.len() != doc.features.len() || self.results.iter().zip(&doc.features).any(|(r, f)| r.id != f.id || r.stale) {
            return false;
        }
        let Some(name) = doc.feature(id).map(|f| f.name.clone()) else { return false };
        let Some(at) = self.results.iter().position(|r| r.id == id) else { return false };
        // An error may quote the old name: the feature's own, or a later one's that refers to it
        // by id (a pattern of it: "{name} cannot be patterned or mirrored yet").
        let old = self.results.get(at).map(|r| r.name.clone()).unwrap_or_default();
        if self.results.iter().skip(at).any(|r| r.error.as_deref().is_some_and(|e| r.id == id || e.contains(&old))) {
            return false;
        }
        if let Some(r) = self.results.get_mut(at) {
            r.name.clone_from(&name);
        }
        let mut renamed: std::collections::HashMap<*const ModelState, Arc<ModelState>> = std::collections::HashMap::new();
        for r in self.results.iter_mut().skip(at) {
            let labels = r.state.sketches.iter().any(|k| k.feature == id) || r.state.construct.iter().any(|c| c.feature == id);
            if !labels {
                continue;
            }
            let state = renamed
                .entry(Arc::as_ptr(&r.state))
                .or_insert_with(|| {
                    let mut st = (*r.state).clone();
                    for k in st.sketches.iter_mut().filter(|k| k.feature == id) {
                        Arc::make_mut(k).name.clone_from(&name);
                    }
                    for c in st.construct.iter_mut().filter(|c| c.feature == id) {
                        c.name.clone_from(&name);
                    }
                    Arc::new(st)
                })
                .clone();
            r.state = state;
        }
        // A note naming the sketch follows its new name.
        for (r, f) in self.results.iter_mut().zip(&doc.features).skip(at) {
            if r.error.is_none() && !r.skipped {
                r.warning = with_retained_notes(doc, r.warning.take(), f, &r.state);
            }
        }
        self.refingerprint(doc);
        true
    }

    /// Set every result's fingerprint to `doc`'s, so the next evaluation reuses every result
    /// (after a change that builds nothing differently).
    pub fn refingerprint(&mut self, doc: &Document) {
        self.refingerprint_from(doc, 0);
    }

    /// How many leading results keep their fingerprints when `doc` has the body names of the
    /// features `renamed` pinned, and nothing else changed (see [`Model::refingerprint_shared`]).
    /// Every command pins, and fingerprinting the whole timeline again for it cost as much as
    /// the fingerprinting of the evaluation itself. When the results hold `doc`'s fingerprints
    /// (the address and digest kept when they were given prove it), those before the first
    /// renamed feature stand, since nothing they read changes, unless a pattern or mirror of
    /// features among them replays a renamed one (it fingerprints its sources' definitions
    /// wherever they are). Without that proof: none.
    pub fn pinned_prefix(&self, doc: &Arc<Document>, renamed: &[u64]) -> usize {
        let proven = self.fingerprinted.is_some_and(|(of, digest)| of == Arc::as_ptr(doc) as usize && digest == fingerprints_digest(&self.results));
        if !proven {
            return 0;
        }
        let Some(first) = doc.features.iter().position(|f| renamed.contains(&f.id)) else { return doc.features.len() };
        let replays_renamed = |f: &Feature| match &f.kind {
            FeatureKind::Pattern { features, bodies, .. } | FeatureKind::Mirror { features, bodies, .. } => {
                bodies.is_empty() && !features.is_empty() && replayed_sources(doc, features).iter().any(|g| renamed.contains(&g.id))
            }
            _ => false,
        };
        doc.features.iter().take(first).position(replays_renamed).unwrap_or(first)
    }

    /// Drop the proof [`Model::pinned_prefix`] reads: the document at that address is about to
    /// change in place.
    pub fn forget_fingerprints(&mut self) {
        self.fingerprinted = None;
    }

    /// Fingerprint the results from `start` on for `doc`, chained onto the fingerprint before.
    pub(crate) fn refingerprint_from(&mut self, doc: &Document, start: usize) {
        self.placement_inputs = None;
        self.fingerprinted = None;
        self.import_digests = ImportDigests::of(doc, &self.import_digests, self.heals());
        let start = start.min(self.results.len()).min(doc.features.len());
        if start == self.results.len().min(doc.features.len()) {
            return;
        }
        let (vals, _) = evaluation_values(doc);
        let marker = doc.marker.unwrap_or(usize::MAX);
        let last = start.checked_sub(1).and_then(|i| self.results.get(i));
        let mut prev = last.map_or(0, |r| r.fingerprint);
        // Each feature's fingerprint reads the model it starts from, as evaluating does.
        let mut before = last.map_or_else(|| Arc::new(ModelState::default()), |r| r.state.clone());
        for (i, (r, f)) in self.results.iter_mut().zip(&doc.features).enumerate().skip(start) {
            prev = chain_fingerprint(doc, &vals, f, prev, i >= marker, &self.import_digests, Some(&before));
            r.fingerprint = prev;
            before = r.state.clone();
        }
    }

    /// Note that the results now hold `doc`'s fingerprints (for [`Model::pinned_prefix`]).
    pub(crate) fn mark_fingerprinted(&mut self, doc: &Arc<Document>) {
        self.fingerprinted = Some((Arc::as_ptr(doc) as usize, fingerprints_digest(&self.results)));
    }
}

/// A digest of the results' features and fingerprints, which proves they are still the ones
/// [`Model::mark_fingerprinted`] saw (whatever changed them since).
fn fingerprints_digest(results: &[FeatureResult]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    results.len().hash(&mut h);
    for r in results {
        r.id.hash(&mut h);
        r.fingerprint.hash(&mut h);
    }
    h.finish()
}

/// Key of the design's length unit (mm per unit) among the values (no parameter can be named
/// so).
pub(crate) const DESIGN_LEN: &str = "\u{1}design_len";

/// An expression's value. A length that comes out unit-less without being a bare number (a
/// unit-less parameter used as a length) is in the design's units.
/// A point given as length expressions.
fn point(vals: &BTreeMap<String, Value>, p: &crate::PointExpr) -> Result<Vec3> {
    let [x, y, z] = p;
    let v = Vec3::new(val(vals, x, Kind::Length)?, val(vals, y, Kind::Length)?, val(vals, z, Kind::Length)?);
    if [v.x, v.y, v.z].iter().all(|c| c.is_finite() && c.abs() < 1e9) { Ok(v) } else { Err(DocError::Invalid("the position is out of range".into())) }
}

/// A primitive's axis (#18822): its components evaluated as unitless numbers, normalized.
fn direction(vals: &BTreeMap<String, Value>, d: &crate::DirExpr) -> Result<Vec3> {
    let [x, y, z] = d;
    let v = Vec3::new(val(vals, x, Kind::Unitless)?, val(vals, y, Kind::Unitless)?, val(vals, z, Kind::Unitless)?);
    if ![v.x, v.y, v.z].iter().all(|c| c.is_finite()) {
        return Err(DocError::Invalid("the axis is not a finite direction".into()));
    }
    crate::unit_direction(v).ok_or_else(|| DocError::Invalid("the axis is zero: give it a direction [x, y, z] that isn't [0, 0, 0]".into()))
}

pub(crate) fn val(vals: &BTreeMap<String, Value>, e: &str, k: Kind) -> Result<f64> {
    val_with(vals, e, k, None)
}

/// [`val`], with the coordinates of points the expression may read (`p1.x`): `members`
/// resolves the point's name and the coordinate (`x` or `y`) to its value.
pub(crate) fn val_with(vals: &BTreeMap<String, Value>, e: &str, k: Kind, members: Option<&dyn Fn(&str, &str) -> Result<Value>>) -> Result<f64> {
    let scale = vals.get(DESIGN_LEN).map(|x| x.v).unwrap_or(1.0);
    let look = |n: &str| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
    // Non-literal lengths (a parameter ± a bare number) must read that bare number in the design's
    // units during the expression, not only when the whole result is unit-less (#9015).
    if k == Kind::Length && scale != 1.0 && !expr::is_literal(e) {
        let defaults = expr::Defaults { len: scale, ..expr::Defaults::default() };
        return expr::eval_with_defaults_members(e, &look, members, defaults)?.to_kind_in(k, defaults);
    }
    expr::eval_with_defaults_members(e, &look, members, expr::Defaults::default())?.to_kind(k)
}

#[cfg(test)]
mod largest_fit_tests {
    use super::*;

    const WHY: &str = "the fillet is larger than the faces beside its edges allow; the largest radius that fits is 1.6 mm";

    /// Nothing smaller builds: the unverified estimate is gone, and the message says what the
    /// smallest candidate met.
    #[test]
    fn a_size_nothing_built_is_not_named() {
        let e = with_largest_fit(WHY.into(), "radius", || {
            Ok(kernel::FitSearch {
                found: None,
                smallest_refused: Some((0.001, kernel::KernelError::TooLarge("the trim leaks; the largest radius that fits is 0.0004 mm".into()))),
                tried: 14,
            })
        })
        .to_string();
        assert!(!e.contains("that fits is"), "{e}");
        assert!(!e.contains("no smaller"), "{e}");
        assert!(e.contains("none of the 14 sizes tried builds (the smallest, 0.001 mm: "), "{e}");
        assert!(e.contains("the trim leaks"), "{e}");
    }

    /// A candidate failing for another reason stops the search: that reason is said, not the
    /// estimate.
    #[test]
    fn a_search_stopped_by_another_failure_says_it() {
        let e = with_largest_fit(WHY.into(), "radius", || Err(kernel::KernelError::Failed("the corner".into()))).to_string();
        assert!(!e.contains("that fits is"), "{e}");
        assert!(e.contains("a smaller radius fails otherwise: "), "{e}");
    }

    /// A refusal naming 0 mm as the largest radius that fits names no size at all: the clause is
    /// dropped, and the rest of the refusal is kept. A size a round can have stays.
    #[test]
    fn a_fit_of_zero_is_not_named() {
        let why = "the fillet is larger than the faces beside its edges allow, at the edge at (0.099, 25.193, 20.000): \
                   the rolling ball does not fit a face it rolls on; the largest radius that fits is 0 mm";
        let e = without_unusable_fit(why);
        assert!(!e.contains("that fits is"), "{e}");
        assert!(e.ends_with("the rolling ball does not fit a face it rolls on"), "{e}");
        assert_eq!(without_unusable_fit(WHY), WHY);
        let plain = "the fillet is larger than the faces beside its edges allow";
        assert_eq!(without_unusable_fit(plain), plain);
    }
}

#[cfg(test)]
mod design_unit_val_tests {
    use super::*;
    use expr::Value;

    /// #9015: with inches as the design unit, `baseLength - 1` subtracts one inch (not one mm).
    #[test]
    fn bare_offsets_beside_a_length_follow_the_design_units() {
        let mut vals = BTreeMap::new();
        vals.insert(DESIGN_LEN.into(), Value::num(25.4));
        vals.insert("baseLength".into(), Value::length(76.0 * 25.4));
        let got = val(&vals, "baseLength - 1", Kind::Length).unwrap();
        assert!((got - 75.0 * 25.4).abs() < 1e-9, "got {got} mm, want 75 in");
        let got = val(&vals, "baseLength - 1.5", Kind::Length).unwrap();
        assert!((got - 74.5 * 25.4).abs() < 1e-9, "got {got} mm, want 74.5 in");
        // An explicit millimetre still wins.
        let got = val(&vals, "baseLength - 1 mm", Kind::Length).unwrap();
        assert!((got - (76.0 * 25.4 - 1.0)).abs() < 1e-9, "got {got}");
        // A bare literal stays millimetres (feature inputs write the design unit in when typed).
        let got = val(&vals, "2", Kind::Length).unwrap();
        assert!((got - 2.0).abs() < 1e-12, "got {got}");
    }
}

/// Collapsing the outline and its holes onto one apex cannot make a manifold solid.
fn refuse_loft_holes(r: &Region2) -> Result<()> {
    if r.holes.is_empty() {
        return Ok(());
    }
    Err(DocError::Invalid("not supported yet: lofting a section with holes to a point".into()))
}

/// What each way of closing a taper's corners ([`kernel::DraftCorner`]) gives at each sharp
/// corner of the profiles `sel` picks in `ss`, extruded `distance` with a taper of `taper` radians
/// (positive grows the profile), the profiles merged as an extrude merges them: per corner, how
/// far each offset curve runs on (or is trimmed), whether the corner cuts in, or why the rule
/// refuses it. A dialog offers the other rules where the chosen one is a poor fit.
pub fn draft_corner_options(ss: &SolvedSketch, sel: &ProfileSel, distance: f64, taper: f64) -> Result<Vec<kernel::CornerReport>> {
    let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, sel)?);
    let mut out = Vec::new();
    for r in &regions {
        out.extend(kernel::draft_corners(r, distance, taper)?);
    }
    Ok(out)
}

/// The corner rule a taper takes for profile `r` of `ss` when the user chose none: preserve
/// curvature, unless a corner of the profile cannot keep it exactly (a curve that is neither a
/// line nor an arc), then the tangent rule, with a note in `notes` saying where and why.
fn draft_corner_for(ss: &SolvedSketch, r: &Region2, distance: f64, taper: f64, notes: &mut Vec<String>) -> Result<kernel::DraftCorner> {
    let reports = kernel::draft_corners(r, distance, taper)?;
    let inexact = reports
        .iter()
        .find(|c| c.options.iter().any(|o| o.rule == kernel::DraftCorner::PreserveCurvature && o.fill == Err(kernel::CornerRefusal::NotExact)));
    Ok(match inexact {
        Some(c) => {
            notes.push(format!(
                "the taper's corners of profile at ({}, {}) in {} use the tangent rule: a curve of it is neither a line nor an arc, so its walls cannot continue as themselves exactly; corner: round also builds",
                show_mm(c.at.x),
                show_mm(c.at.y),
                ss.name
            ));
            kernel::DraftCorner::Tangent
        }
        None => kernel::DraftCorner::PreserveCurvature,
    })
}

pub fn select_profiles(ss: &SolvedSketch, sel: &ProfileSel) -> Result<Vec<Region2>> {
    let ps = &ss.profiles;
    if ps.is_empty() {
        return Err(no_closed_profiles(ss));
    }
    let out: Vec<Region2> = match sel {
        // "All" means what was drawn: projected reference geometry (a face's auto-projected
        // edges) only counts when the drawn curves alone make no profile. Text is drawn, and
        // its letters keep their counters as holes (the counters are not filled in).
        ProfileSel::All => {
            let drawn = solidcraft_sketch::find_drawn_profiles(&ss.sketch);
            let chosen = if drawn.is_empty() { ps.clone() } else { drawn };
            solidcraft_sketch::without_text_counters(&ss.sketch, chosen).into_iter().map(|p| p.region).collect()
        }
        ProfileSel::Indices { indices } => {
            // A design file may list a profile more than once; it is still one profile.
            let mut seen = std::collections::BTreeSet::new();
            let mut v = Vec::new();
            for i in indices.iter().filter(|i| seen.insert(**i)) {
                v.push(ps.get(*i).ok_or_else(|| DocError::Unknown(format!("profile {i} of {}", ss.name)))?.region.clone());
            }
            v
        }
        // A curve loop that no longer bounds a profile exactly falls back to the best match; the
        // feature's warning says so (`profile_fallback_notes`).
        ProfileSel::Curves { .. } | ProfileSel::OffsetBoundaries { .. } | ProfileSel::WholeBoundaries { .. } => {
            crate::profile_selection_indices_lenient(ss, sel)?
                .0
                .into_iter()
                .map(|index| {
                    ps.get(index)
                        .map(|profile| profile.region.clone())
                        .ok_or_else(|| DocError::Invalid("profile index changed during resolution".into()))
                })
                .collect::<Result<Vec<_>>>()?
        }
        ProfileSel::Points { points } => {
            let mut v = Vec::new();
            for p in points {
                // Innermost profile containing the point.
                let found = solidcraft_sketch::profile_at_point(ps, *p).and_then(|i| ps.get(i));
                v.push(found.ok_or_else(|| DocError::Unknown(format!("profile at {:?} in {}", [p.x, p.y], ss.name)))?.region.clone());
            }
            v
        }
    };
    if out.is_empty() {
        return Err(DocError::Invalid("no profile selected".into()));
    }
    Ok(out)
}

/// The error for a profile picked by curves that no longer names exactly one profile.
pub fn curve_pick_error(e: solidcraft_sketch::CurvePick, ids: &[String], sketch: &str) -> DocError {
    match e {
        solidcraft_sketch::CurvePick::Missing => DocError::Invalid(format!(
            "no profile of {sketch} is bounded by exactly the curves {ids:?} (a curve was deleted, or another curve now cuts the region)"
        )),
        solidcraft_sketch::CurvePick::Ambiguous(n) => DocError::Invalid(format!(
            "{n} profiles of {sketch} are bounded by exactly the curves {ids:?}; add a curve that tells them apart, or pick by index or point"
        )),
    }
}

pub(crate) fn new_name(state: &mut ModelState, f: &Feature, k: usize) -> String {
    if let Some(n) = f.body_names.get(k)
        && !n.is_empty()
        && state.body(n).is_none()
    {
        // A pinned `BodyN` counts as handed out, so a design evaluated again from its stored
        // names ends with the counter it had when the counter numbered them.
        if let Some(number) = n.strip_prefix("Body").filter(|_| crate::is_counted_body_name(n)).and_then(|d| d.parse::<usize>().ok()) {
            state.body_counter = state.body_counter.max(number);
        }
        return n.clone();
    }
    // Bodies past the stored names (a profile added after the feature was made) are numbered
    // after the first name: `Ribs (4)` (#373). Features saved before format 14 keep `BodyN`,
    // which later features of those designs may name.
    if !f.shell_piece_order
        && k >= f.body_names.len()
        && let Some(base) = f.body_names.first().map(|n| n.trim()).filter(|n| !n.is_empty())
    {
        let n = format!("{base} ({})", k.saturating_add(1));
        return if state.body(&n).is_none() { n } else { unique_body_name(state, base) };
    }
    loop {
        state.body_counter += 1;
        let n = format!("Body{}", state.body_counter);
        // A name a rename freed is not handed out again (#4957).
        if state.body(&n).is_none() && !state.reserved_body_names.contains(&n) {
            return n;
        }
    }
}

/// Where in a body Pattern's `body_names` the name of copy `k` of source `s` is kept, for a
/// pattern of `sources` bodies with `copies` copies each. The names are kept source by source,
/// and the session keeps one name for every copy of a pattern of several bodies
/// (`Session::pin_body_names`), so the stored names split evenly into the copies each source had
/// when they were stored. When the count changes, a copy keeps the name stored for its own source
/// and number, and a new copy has none (#12114). Names that don't split evenly (stored before
/// every copy was named), and names of a design saved before then (`in_order`, see
/// `Feature::pattern_names_in_order`), are read in the order the copies are made.
pub(crate) fn pattern_name_slot(stored: usize, sources: usize, copies: usize, s: usize, k: usize, in_order: bool) -> Option<usize> {
    if !in_order && sources > 1 && stored > 0 && stored.is_multiple_of(sources) {
        let per = stored / sources;
        return (k < per).then(|| s.saturating_mul(per).saturating_add(k));
    }
    s.checked_mul(copies)?.checked_add(k)
}

/// A copy of a body pattern at `slot` of its stored names: its stored name, else a copy of
/// `source`. Once `body.rename` has stored this feature's names, a copy beyond them (the count
/// was raised) is numbered after the highest stored `source (n)`, so it never takes a number
/// freed by a rename (#372). Features without stored names keep the first free `source (n)`.
fn pattern_copy_name(names: &mut BodyNames, f: &Feature, slot: Option<usize>, source: &str) -> String {
    if let Some(k) = slot
        && let Some(name) = f.body_names.get(k).filter(|name| !name.trim().is_empty())
    {
        return names.unique(name);
    }
    let base = if source.trim().is_empty() { "Body" } else { source.trim() };
    let prefix = format!("{base} (");
    let highest = f.body_names.iter().filter_map(|n| n.trim().strip_prefix(prefix.as_str())?.strip_suffix(')')?.parse::<u32>().ok()).max();
    match highest {
        Some(h) => names.first_free(base, u64::from(h) + 1),
        None => names.unique(base),
    }
}

fn copy_body_name(state: &ModelState, f: &Feature, k: usize, source: &str) -> String {
    let base = f.body_names.get(k).filter(|name| !name.trim().is_empty()).map(String::as_str).unwrap_or(source);
    unique_body_name(state, base)
}

/// Reads what a feature cut from a sheet made from surfaces back onto its sheet, and puts the
/// body built again from the sheet in place (#6748, see `sheet_surface_cuts`). Checking each
/// step, the body built again is weighed against the body cut.
fn absorb_sheet_cuts(
    doc: &Document,
    before: &ModelState,
    after: &mut ModelState,
    f: &Feature,
    validation: kernel::Validation,
    warning: &mut Option<String>,
) -> Result<()> {
    let weigh = validation == kernel::Validation::EachStep;
    let letters = more::cut_letters(after, f);
    // What takes material away that the sheet can't hold (a cut, a hole, a chamfer or fillet on
    // a face's edge) is refused; any other change (a join, a mirror into it, a modelled thread)
    // stays on the body, and marks the sheet (see `SurfaceSheet::altered_by`).
    let cuts = takes_material(doc, f, 0);
    // What isn't a cut is always weighed: material the sheet doesn't show (a join into a bend)
    // is caught there.
    let drawn = after.cut_extents.get(&f.id).copied();
    for (i, absorbed) in crate::sheet_surface_cuts::absorb(before, after, f, weigh || !cuts, drawn.as_ref())? {
        let (mut sk, built) = match absorbed {
            crate::sheet_surface_cuts::Absorbed::Read(sk, built) => (*sk, built),
            // An intersect keeps what lies in its tool and takes the rest: refused, it says what
            // the sheet can read back instead.
            crate::sheet_surface_cuts::Absorbed::Refused(why)
                if cuts && matches!(&f.kind, FeatureKind::Extrude { operation: Operation::Intersect, .. }) =>
            {
                return Err(DocError::Invalid(format!(
                    "{why}; this sheet machining cannot be read back onto the sheet: use a parallel extrude, or unfold the sheet before intersecting"
                )));
            }
            crate::sheet_surface_cuts::Absorbed::Refused(why) if cuts => return Err(DocError::Invalid(why)),
            crate::sheet_surface_cuts::Absorbed::Refused(_) => {
                // Its body is no longer as it built it: what it said of the body's faces is gone.
                if let Some(sk) = after.surface_sheets.get_mut(i) {
                    sk.parts.clear();
                }
                if let Some(sk) = after.surface_sheets.get_mut(i).filter(|sk| sk.altered_by.is_none()) {
                    sk.altered_by = Some(f.name.clone());
                    append_warning(
                        warning,
                        format!(
                            "`{}` is sheet metal: its flat pattern, DXF and bend table can't include this change yet and are refused; a later sheet metal feature rebuilds the body without it",
                            sk.body
                        ),
                    );
                }
                continue;
            }
        };
        // The sheet remembers where the cut's letters went.
        if let Some(flat) = letters.as_ref().and_then(|(plane, regions)| sk.flat_of(plane, regions)) {
            sk.labels.extend(flat);
        }
        // Its faces named from what the sheet says each was made for, and what it said of the body
        // the feature found (see `lineage::sheet_lineage`).
        let lineage = match (before.body(&sk.body), before.surface_sheets.iter().find(|s| s.body == sk.body)) {
            (Some(mb), Some(old)) => crate::lineage::sheet_lineage(Some(crate::lineage::SheetBefore::found(mb, &old.parts)), &sk, &built),
            _ => None,
        };
        if let Some(b) = after.bodies.iter_mut().find(|b| b.name == sk.body) {
            more::replace_traced_sheet_body(b, built, lineage, &sk.frames);
        }
        if let Some(slot) = after.surface_sheets.get_mut(i) {
            *slot = sk;
        }
    }
    Ok(())
}

/// Whether feature `f` takes material away (see [`absorb_sheet_cuts`]): a hole, a chamfer or
/// fillet, and a cut or intersect, a Combine's among them. A pattern or mirror of features takes
/// material away when a feature it copies does, through patterns of patterns `depth` deep; one
/// of bodies copies them.
fn takes_material(doc: &Document, f: &Feature, depth: usize) -> bool {
    match &f.kind {
        FeatureKind::Hole { .. } | FeatureKind::Chamfer { .. } | FeatureKind::Fillet { .. } => true,
        FeatureKind::Extrude { operation, .. }
        | FeatureKind::Revolve { operation, .. }
        | FeatureKind::Box { operation, .. }
        | FeatureKind::Cylinder { operation, .. }
        | FeatureKind::Sphere { operation, .. }
        | FeatureKind::Torus { operation, .. }
        | FeatureKind::Cone { operation, .. }
        | FeatureKind::Loft { operation, .. }
        | FeatureKind::Sweep { operation, .. }
        | FeatureKind::Pipe { operation, .. }
        | FeatureKind::Coil { operation, .. }
        | FeatureKind::BoundaryFill { operation, .. }
        | FeatureKind::Thicken { operation, .. }
        | FeatureKind::SurfaceNetwork { operation, .. }
        | FeatureKind::Combine { operation, .. } => matches!(operation, Operation::Cut | Operation::Intersect),
        FeatureKind::Pattern { features, bodies, .. } | FeatureKind::Mirror { features, bodies, .. } if bodies.is_empty() => {
            depth < MAX_NESTING && features.iter().filter_map(|id| doc.feature(id.0)).any(|g| !g.suppressed && takes_material(doc, g, depth + 1))
        }
        _ => false,
    }
}

/// Keeps each sheet metal body's flat pattern true to the body (see `SurfaceSheet::altered_by`,
/// which [`absorb_sheet_cuts`] sets for a change the sheet can't hold). A feature that rebuilds
/// the body from its sheet (a sheet metal feature, or one that changes the sheet) clears the
/// mark, and says that the change it drops is not kept. Features that move whole bodies or only
/// mark their faces (a cosmetic thread, not a modelled one, which cuts material) leave the flat
/// pattern as it was.
fn track_altered_sheets(before: &ModelState, after: &mut ModelState, f: &Feature, warning: &mut Option<String>) {
    if matches!(
        f.kind,
        FeatureKind::Move { .. }
            | FeatureKind::Align { .. }
            | FeatureKind::Rest { .. }
            | FeatureKind::BodyTransfer { .. }
            | FeatureKind::SplitFace { .. }
            | FeatureKind::Thread { modeled: false, .. }
    ) {
        return;
    }
    for sk in after.surface_sheets.iter_mut() {
        let Some(old) = before.surface_sheets.iter().find(|s| s.body == sk.body) else { continue };
        if old.altered_by.is_some()
            && (f.kind.is_sheet() || old.sheet != sk.sheet)
            && let Some(lost) = sk.altered_by.take()
        {
            append_warning(
                warning,
                format!("`{}` is rebuilt from its sheet metal features: what {lost} changed is not kept (not supported yet)", sk.body),
            );
        }
    }
}

/// A Join whose material meets none of its targets still joins it, to the first target as a
/// separate piece (see `apply_traced`): a valid body in several pieces, but rarely what was meant
/// when Join was left on by mistake. Says so when a feature's Join leaves the bodies it changed
/// in more pieces than they had (#4434). Combine joins the bodies it is given, apart or not.
fn note_join_apart(doc: &Document, before: &ModelState, after: &ModelState, f: &Feature, warning: &mut Option<String>) {
    let mirrored_join = matches!(&f.kind, FeatureKind::Mirror { features, bodies, .. }
    if bodies.is_empty() && features.iter().filter_map(|id| doc.feature(id.0)).any(|source| {
        !source.suppressed && matches!(source.kind, FeatureKind::Extrude { operation: Operation::Join, .. })
    }));
    if feature_operation(f) != Some(Operation::Join) && !mirrored_join {
        return;
    }
    for (name, n) in join_pieces_added(before, after) {
        append_warning(
            warning,
            format!("the joined piece doesn't touch `{name}`, so `{name}` is now in {n} separate pieces (New Body makes it a body of its own)"),
        );
    }
}

/// A Cut whose tool never reaches the outside of a body leaves a closed cavity in it: valid, but
/// rarely meant (a pocket whose tool was shortened until it stopped short of the face, #7317).
/// Says so, with the volume the cut took, when a feature's Cut leaves a body with more closed
/// voids than it had.
fn note_cut_cavity(before: &ModelState, after: &ModelState, f: &Feature, warning: &mut Option<String>) {
    if feature_operation(f) != Some(Operation::Cut) {
        return;
    }
    for was in &before.bodies {
        let Some(now) = after.body(&was.name) else { continue };
        // Only a body with more shells can have gained a void, and it is the cheap test.
        if now.body.shell_count() <= was.body.shell_count() || now.body.same_shape(&was.body) || voids(&now.body) <= voids(&was.body) {
            continue;
        }
        // The exact volumes before and after: what the cut took, the cavity when it took nothing else.
        let removed = match (kernel::measure_mass(&was.body), kernel::measure_mass(&now.body)) {
            (Ok(a), Ok(b)) if (a.volume - b.volume).is_finite() && a.volume > b.volume => {
                format!(", taking {} mm³", show_mm(((a.volume - b.volume) * 100.0).round() / 100.0))
            }
            _ => String::new(),
        };
        append_warning(
            warning,
            format!(
                "the cut leaves a closed cavity inside `{}` that opens on no face{removed} (extend the tool through the face to open it)",
                now.name
            ),
        );
    }
}

/// The closed voids of a body: its shells beyond the one outside each of its pieces.
fn voids(b: &Body) -> usize {
    b.shell_count().saturating_sub(b.lumps().map_or(1, |l| l.len()))
}

/// The operation of a feature that makes or changes bodies with one.
pub fn feature_operation(f: &Feature) -> Option<Operation> {
    Some(match &f.kind {
        FeatureKind::Extrude { operation, .. }
        | FeatureKind::Revolve { operation, .. }
        | FeatureKind::Box { operation, .. }
        | FeatureKind::Cylinder { operation, .. }
        | FeatureKind::Sphere { operation, .. }
        | FeatureKind::Torus { operation, .. }
        | FeatureKind::Cone { operation, .. }
        | FeatureKind::Loft { operation, .. }
        | FeatureKind::Sweep { operation, .. }
        | FeatureKind::Pipe { operation, .. }
        | FeatureKind::Coil { operation, .. }
        | FeatureKind::BoundaryFill { operation, .. }
        | FeatureKind::Thicken { operation, .. }
        | FeatureKind::SurfaceNetwork { operation, .. } => *operation,
        _ => return None,
    })
}

/// The bodies a Join from `before` to `after` left in more separate pieces than it found, with
/// their piece counts now: empty when the joined material made one body with what it touched
/// (or bridged pieces). A lid closing a cavity adds a void, not a piece.
pub fn join_pieces_added(before: &ModelState, after: &ModelState) -> Vec<(String, usize)> {
    let pieces = |b: &Body| b.lumps().map_or(1, |l| l.len());
    // Pieces of the bodies the join changed or absorbed before it, and of those it kept after.
    let (mut had, mut have) = (0usize, 0usize);
    let mut grown: Vec<(String, usize)> = Vec::new();
    for was in &before.bodies {
        match after.body(&was.name) {
            Some(now) if now.body.same_shape(&was.body) => {}
            Some(now) => {
                let (old, new) = (pieces(&was.body), pieces(&now.body));
                had += old;
                have += new;
                if new > old {
                    grown.push((now.name.clone(), new));
                }
            }
            None => had += pieces(&was.body),
        }
    }
    if have <= had { Vec::new() } else { grown }
}

/// Selected profiles form one machining tool, including disconnected tool material: their
/// union in one kernel call.
fn intersection_tool_union(tools: Vec<Traced>) -> Result<Vec<Traced>> {
    if tools.len() > 1000 || tools.iter().fold(0usize, |n, t| n.saturating_add(t.body.face_count())) > 16_384 {
        return Err(DocError::Invalid("intersection tools exceed the bounded geometry budget".into()));
    }
    if tools.len() < 2 {
        return Ok(tools);
    }
    Ok(vec![union_all(&tools.iter().collect::<Vec<_>>())?])
}

/// Refuses a cut or intersect of `tools` that would change the sheet's body `body`, for machining
/// the sheet can't read back: a cut is a 3D boolean on the folded body, which the sheet reads back
/// onto its panels square through them or as a pocket, so walls not square to the sheet have no
/// place on it. A cut that meets the body changes it, as does an intersect whose tools leave some
/// of it out.
fn sheet_boolean_unchanged(body: &Body, tools: &[Body], op: Operation) -> Result<()> {
    let united;
    let tools: &[Body] = if op == Operation::Intersect {
        united = intersection_tool_union(tools.iter().cloned().map(Traced::unnamed).collect())?.into_iter().map(|t| t.body).collect::<Vec<_>>();
        &united
    } else {
        tools
    };
    let test = if op == Operation::Cut { BoolOp::Intersect } else { BoolOp::Cut };
    for tool in tools {
        if kernel::boolean(body, tool, test)?.is_some() {
            return Err(DocError::Invalid("this sheet machining cannot be read back onto the sheet: its walls are not square to the sheet (a tapered cut that stops inside a panel's thickness, or a tapered intersect); use a parallel, untapered extrude cut or pocket, or unfold the sheet before intersecting".into()));
        }
    }
    Ok(())
}

/// The union of `operands` (traced bodies) in one kernel call, keeping their lineage.
fn union_all(operands: &[&Traced]) -> Result<Traced> {
    let bodies: Vec<&Body> = operands.iter().map(|t| &t.body).collect();
    Ok(Traced::step(kernel::union_all(&bodies)?, operands))
}

/// The union of `operands` when no two of them meet or touch: the bodies side by side as one body
/// of several pieces, keeping their lineage, with no intersection work. They are apart when
/// their exact boxes, each grown by twice its body's tolerance, do not meet
/// ([`kernel::union_apart`]). `None` when that cannot be shown (two boxes meet, a mesh body, a
/// box that cannot be measured): the caller runs the kernel's union.
fn union_all_apart(operands: &[&Traced]) -> Option<Traced> {
    let bodies: Vec<Body> = operands.iter().map(|t| t.body.clone()).collect();
    let one = kernel::union_apart(&bodies).ok().flatten()?;
    #[cfg(test)]
    cost_tests::count(|c| c.joined_apart += 1);
    Some(Traced::step(one, operands))
}

/// After a cut handled some sheet bodies as flat machining, apply it to the other targets.
fn apply_beside_sheet_cut(
    doc: &Document,
    st: &mut ModelState,
    f: &Feature,
    tools: Vec<Traced>,
    op: Operation,
    targets: &[String],
    handled: &[String],
) -> Result<()> {
    let effective = if targets.is_empty() { &f.participants } else { targets };
    let all = effective.len() == 1 && effective.first().is_some_and(|t| t == "*");
    // Some sheet solids have already been cut, possibly by another pattern group. Preserve
    // automatic surfaces before passing the rest as names, which would otherwise refuse them.
    let skip_surfaces = op == Operation::Cut && (all || effective.is_empty());
    let rest: Vec<String> = st
        .bodies
        .iter()
        .filter(|b| {
            !handled.contains(&b.name)
                && (!skip_surfaces || !b.is_surface())
                && (if all {
                    !b.envelope
                } else if effective.is_empty() {
                    b.in_default_scope(f)
                } else {
                    effective.contains(&b.name)
                })
        })
        .map(|b| b.name.clone())
        .collect();
    if rest.is_empty() {
        return Ok(());
    }
    apply_traced(doc, st, f, tools, op, &rest)
}

fn apply_op(doc: &Document, state: &mut ModelState, f: &Feature, tools: Vec<Body>, op: Operation, targets: &[String]) -> Result<()> {
    apply_traced(doc, state, f, tools.into_iter().map(Traced::unnamed).collect(), op, targets)
}

/// The most cut tools [`clear_tools_as_one`] joins into one tool. The kernel's boolean takes
/// far longer for a tool of many pieces than for several of fewer: one cut of 20 apart pieces
/// into a ring took 0.34 s, of 40 1.75 s, of 60 6.4 s, of about 105 (a pattern of a pattern,
/// #8765) 23 s. So more tools are joined in groups of at most this many, each cut in turn. This
/// works around the boolean's cost; it does not change what is cut.
const MAX_JOINED_TOOLS: usize = 32;

#[cfg(test)]
thread_local! {
    /// [`MAX_JOINED_TOOLS`] for this thread's tests: a test cuts the same tools in one go.
    static JOINED_TOOLS_LIMIT: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_JOINED_TOOLS) };
}

#[cfg(test)]
fn joined_tools_limit() -> usize {
    JOINED_TOOLS_LIMIT.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn joined_tools_limit() -> usize {
    MAX_JOINED_TOOLS
}

/// The recipe of a tool a primitive or coil feature `f` builds: its definition and the values it
/// uses fix it.
fn tool_recipe(doc: &Document, f: &Feature, vals: &BTreeMap<String, Value>) -> Option<u64> {
    // A cylinder placed by reference edges stands on the model, not on its definition alone.
    if !hole_references(f).is_empty() {
        return None;
    }
    Some(fingerprint(doc, 0, f, vals, false, &ImportDigests::default()))
}

/// The recipe of a tool of recipe `recipe` moved by `m`: copy `instance` of feature `feature`
/// (its faces are named after the copy).
fn moved_recipe(recipe: u64, m: &Mat, feature: u64, instance: usize) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (recipe, feature, instance).hash(&mut h);
    m.iter().flatten().for_each(|x| x.to_bits().hash(&mut h));
    h.finish()
}

/// The recipe of `tools` together, when each has one.
fn recipe_of(tools: &[Traced]) -> Option<u64> {
    let recipes = tools.iter().map(|t| t.recipe).collect::<Option<Vec<u64>>>()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    recipes.hash(&mut h);
    Some(h.finish())
}

/// Many cut tools clear of each other (a pattern's copies, the circles of a vent field in one
/// sketch) as one tool of many pieces, so the target is cut once instead of rebuilding an ever
/// more complex body for every tool (#261, #368). Tools that may meet are left as they are.
/// More than [`MAX_JOINED_TOOLS`] tools are joined in even groups, one tool for each (#8765).
fn clear_tools_as_one(tools: Vec<Traced>) -> Result<Vec<Traced>> {
    let limit = joined_tools_limit().max(1);
    if tools.len() <= limit {
        return clear_group_as_one(tools);
    }
    let groups = tools.len().div_ceil(limit);
    let size = tools.len().div_ceil(groups);
    let mut rest = tools.into_iter();
    let mut out = Vec::new();
    loop {
        let group: Vec<Traced> = rest.by_ref().take(size).collect();
        if group.is_empty() {
            return Ok(out);
        }
        out.extend(clear_group_as_one(group)?);
    }
}

/// [`clear_tools_as_one`] for one group of tools.
fn clear_group_as_one(tools: Vec<Traced>) -> Result<Vec<Traced>> {
    if tools.len() < 8 {
        return Ok(tools);
    }
    let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
    let one = match kernel::union_apart(&bodies)? {
        Some(one) => Some(one),
        // The kernel only compares the tools' boxes, so copies of a through hole close together
        // on a circle (their boxes overlap) seem to meet (#9007). Their exact shapes may still
        // be apart.
        None if tools_clearly_apart(&bodies) => kernel::join_apart(&bodies).ok().flatten(),
        None => None,
    };
    #[cfg(test)]
    cost_tests::count(|c| c.cut_tools += one.as_ref().map_or(tools.len(), |_| 1));
    Ok(match one {
        // Each piece's faces keep the names of the tool it is.
        Some(one) => {
            let recipe = recipe_of(&tools);
            vec![Traced::step(one, &tools.iter().collect::<Vec<_>>()).with_recipe(recipe)]
        }
        None => tools,
    })
}

/// Most pairs of tools whose boxes meet that [`tools_clearly_apart`] tests.
const MAX_CLOSE_TOOL_PAIRS: usize = 4096;

/// Whether no two of `bodies` meet or touch, decided on their exact shapes. Only pairs whose
/// exact boxes, grown by twice the bodies' tolerances (as [`kernel::apart`] grows them), meet
/// are tested. All of the bodies of those pairs are tested together: their union comes out with
/// as many material regions as there are bodies of them, and no two of those regions meet.
/// Crossing and nested bodies come out in one piece, touching ones in one or in two that share
/// an edge or a face.
/// One union of all of them replaces a union for every pair of them: a pattern of fifty copies
/// of a coil cut took a union for each of its pairs, and minutes (#17632).
/// `false` whenever that cannot be shown: a mesh body, a body without an exact box, too many
/// pairs to test, a union that fails.
fn tools_clearly_apart(bodies: &[Body]) -> bool {
    if bodies.len() < 2 || bodies.len() > 10_000 || bodies.iter().any(Body::is_mesh) {
        return false;
    }
    let Some(boxes) = bodies.iter().map(more::exact_box).collect::<Option<Vec<_>>>() else { return false };
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&i, &j| boxes.get(i).map_or(0.0, |b| b.min.x).total_cmp(&boxes.get(j).map_or(0.0, |b| b.min.x)));
    let mut close = Vec::new();
    for (k, &i) in order.iter().enumerate() {
        let Some(a) = boxes.get(i) else { return false };
        for &j in order.iter().skip(k + 1) {
            let Some(b) = boxes.get(j) else { return false };
            if b.min.x > a.max.x {
                break;
            }
            if more::boxes_apart(&Some(*a), &Some(*b)) {
                continue;
            }
            if close.len() == MAX_CLOSE_TOOL_PAIRS {
                return false;
            }
            close.push((i, j));
        }
    }
    if close.is_empty() {
        // No two of them meet: they are apart.
        return true;
    }
    // The bodies of those pairs: the union of all of them says whether they are apart.
    let mut in_pair = vec![false; bodies.len()];
    for &(i, j) in &close {
        in_pair[i] = true;
        in_pair[j] = true;
    }
    let group: Vec<&Body> = (0..bodies.len()).filter(|&i| in_pair[i]).map(|i| &bodies[i]).collect();
    #[cfg(test)]
    cost_tests::count(|c| c.apart_unions += 1);
    match kernel::union_all(&group) {
        // One union of all of them: it comes out with a piece for each of them when no two of
        // them meet or touch.
        Ok(one) => one.record().material_regions().len() == group.len() && one.region_contacts().is_ok_and(|c| c.is_empty()),
        // The union of all of them failed: each pair is tried on its own, as before.
        Err(_) => close.iter().all(|&(i, j)| matches!((bodies.get(i), bodies.get(j)), (Some(a), Some(b)) if apart_by_union(a, b))),
    }
}

/// Whether the exact union of `a` and `b` comes out in two pieces that share no face, edge or
/// vertex. Crossing and nested bodies come out in one piece, touching ones in one or in two that
/// share an edge or a face. The fallback of [`tools_clearly_apart`], for a union of all the
/// bodies that failed.
fn apart_by_union(a: &Body, b: &Body) -> bool {
    #[cfg(test)]
    cost_tests::count(|c| c.apart_unions += 1);
    match kernel::boolean(a, b, BoolOp::Union) {
        Ok(Some(u)) => u.record().material_regions().len() == 2 && u.region_contacts().is_ok_and(|c| c.is_empty()),
        _ => false,
    }
}

/// An intersect never deletes a body by accident (#5295): it is refused when nothing of a named
/// target would remain, when nothing of any target would remain, or when the tools meet a target
/// and the boolean still came out empty. Only an unnamed target the tools miss altogether is
/// removed, as an intersect with every body does (#5160).
pub(crate) fn empty_intersection(bodies: &[&str]) -> DocError {
    let body = match bodies {
        [one] => format!("`{one}`"),
        [first, rest @ ..] => format!("`{first}` and {} more", rest.len()),
        [] => "the target body".to_string(),
    };
    DocError::Invalid(format!("the intersection is empty: nothing of {body} would remain; check that the profile overlaps the body"))
}

thread_local! {
    /// What the feature being evaluated says about targets it found joined into another body
    /// ([`joined_into`]); [`eval_feature`] adds them to its warning.
    static JOINED_TARGETS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The heals of the import being evaluated, as its fingerprint has them
    /// ([`ImportDigests::heals`]); set by [`Model::evaluate`] for that one feature.
    static IMPORT_HEALS: std::cell::RefCell<Option<Arc<kernel::ImportHeals>>> = const { std::cell::RefCell::new(None) };
}

/// The body that the target `name`, gone from the model, is part of now: a Combine before `f`
/// joined it into its target, which a later Combine may have joined on in turn (#17807). `None`
/// when no such Combine took it (a cut or intersect consumes its tools: they are part of
/// nothing), or the body it went into is gone too.
fn joined_into<'d>(doc: &'d Document, state: &ModelState, f: &Feature, name: &str) -> Option<&'d str> {
    let mut now: Option<&'d str> = None;
    for g in doc.features.iter().take_while(|g| g.id != f.id).filter(|g| !g.suppressed) {
        if let FeatureKind::Combine { target, tools, operation: Operation::Join | Operation::NewBody, keep_tools: false } = &g.kind
            && tools.iter().any(|t| t.0.as_str() == now.unwrap_or(name))
        {
            now = Some(target.0.as_str());
        }
    }
    now.filter(|b| state.body(b).is_some())
}

/// The body a feature's target name `name` picks. Body names are unique, so a body of the
/// feature's component asked to be called a name another component's body already has (a
/// linked component's `AS5047P_PCB`) was named otherwise (`Body1`), and the name still picks the
/// other component's body. That is refused with both bodies listed, instead of running the
/// boolean on the other component's body and failing there with a kernel error (#12141).
pub(crate) fn named_target(doc: &Document, state: &ModelState, f: &Feature, name: &str) -> Result<usize> {
    let i = state.bodies.iter().position(|b| b.name == name).ok_or_else(|| DocError::Unknown(format!("body `{name}`")))?;
    let Some(found) = state.bodies.get(i).filter(|b| b.component != f.component) else { return Ok(i) };
    let asked: Vec<&str> =
        asked_names(doc, state).filter(|(b, asked)| b.component == f.component && *asked == Some(name)).map(|(b, _)| b.name.as_str()).collect();
    if asked.is_empty() {
        return Ok(i);
    }
    let component = |id: u64| doc.components.iter().find(|c| c.id == id).map_or_else(|| "the root".to_string(), |c| format!("`{}`", c.name));
    Err(DocError::Invalid(format!(
        "the body name `{name}` is ambiguous: it names the body of component {}, while this feature's component {} has {} that was asked to be called `{name}` (the name was taken); target {} or rename the bodies so each name is used once",
        component(found.component),
        component(f.component),
        asked.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", "),
        asked.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(" or "),
    )))
}

/// Each body with the name it was asked to be called: its slot of its feature's stored names.
/// An Import's skipped solids make no body, so its bodies take the slots left, in order; any
/// other feature's k-th body takes slot k. Siblings a later feature removed shift the count, so
/// a body then reads a later slot's name (only ever refusing an ambiguous target). One pass
/// over the bodies, each feature's slots listed once, so a large import costs linear time.
fn asked_names<'a, 's>(doc: &'a Document, state: &'s ModelState) -> impl Iterator<Item = (&'s ModelBody, Option<&'a str>)> {
    let mut made: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    let mut slots: std::collections::HashMap<u64, Vec<usize>> = std::collections::HashMap::new();
    state.bodies.iter().map(move |b| {
        let j = made.entry(b.feature).or_insert(0);
        let k = *j;
        *j += 1;
        let asked = doc.feature(b.feature).and_then(|m| {
            let slot = match &m.kind {
                FeatureKind::Import { skip, .. } => *slots
                    .entry(m.id)
                    .or_insert_with(|| {
                        let skip: std::collections::HashSet<u64> = skip.iter().copied().collect();
                        (0..m.body_names.len()).filter(|s| !skip.contains(&(*s as u64))).collect()
                    })
                    .get(k)?,
                _ => k,
            };
            m.body_names.get(slot).map(String::as_str).filter(|n| !n.is_empty())
        });
        (b, asked)
    })
}

/// Refuse the target names among `names` that [`named_target`] finds ambiguous, before any
/// work that does not go through it (a sheet's cut-outs are machined before the boolean). Names
/// that pick no body are left to the feature, which may allow them.
fn refuse_ambiguous_targets(doc: &Document, state: &ModelState, f: &Feature, names: &[String]) -> Result<()> {
    for name in names.iter().filter(|n| *n != "*") {
        if let Err(e @ DocError::Invalid(_)) = named_target(doc, state, f, name) {
            return Err(e);
        }
    }
    Ok(())
}

/// The bodies a feature's boolean takes part in, and whether they were named rather than taken
/// by default (every body, or the feature's component).
pub(crate) fn boolean_scope(state: &ModelState, f: &Feature, targets: &[String]) -> (bool, Vec<String>) {
    let effective = if targets.is_empty() { &f.participants } else { targets };
    let all = effective.len() == 1 && effective.first().is_some_and(|t| t == "*");
    let names = state
        .bodies
        .iter()
        .filter(|b| {
            if all {
                !b.envelope
            } else if effective.is_empty() {
                b.in_default_scope(f)
            } else {
                effective.contains(&b.name)
            }
        })
        .map(|b| b.name.clone())
        .collect();
    (!all && !effective.is_empty(), names)
}

/// Suggest one nearby body without changing the default component scope or running a boolean.
fn empty_boolean_targets(doc: &Document, state: &ModelState, f: &Feature, tools: &[Traced], default_targets: bool) -> DocError {
    let plain = "there is no body to cut or intersect";
    if !default_targets {
        return DocError::Invalid(plain.into());
    }
    let component_name = |id| {
        if id == 0 {
            doc.name.clone()
        } else {
            doc.components.iter().take(10_000).find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_else(|| format!("Component{id}"))
        }
    };
    let tool_bounds: Vec<_> = tools.iter().take(64).map(|t| t.body.outline_bounds()).collect();
    let frame = doc.component_transform(f.component);
    let Some(inverse) = crate::mat_inverse(&frame) else { return DocError::Invalid(plain.into()) };
    // Named by the component the Browser shows the body in; one shown in the feature's own
    // component has no other component to point at.
    let others = state
        .bodies
        .iter()
        .take(256)
        .map(|b| (b, doc.model_body_component(b)))
        .filter(|(b, owner)| b.component != f.component && *owner != f.component);
    for (body, owner) in others {
        // Compare in the feature's frame, including component placements and body offsets.
        let world = body_world(doc, body);
        let relative = crate::mat_mul(&inverse, &world);
        let bounds = body.outline_bounds();
        if bounds.is_empty() || !bounds.min.is_finite() || !bounds.max.is_finite() {
            continue;
        }
        let mut placed = solidcraft_geom::Aabb3::EMPTY;
        for x in [bounds.min.x, bounds.max.x] {
            for y in [bounds.min.y, bounds.max.y] {
                for z in [bounds.min.z, bounds.max.z] {
                    placed.add(crate::apply_point(&relative, Vec3::new(x, y, z)));
                }
            }
        }
        if tool_bounds.iter().any(|b| !b.is_empty() && b.min.is_finite() && b.max.is_finite() && b.meets(&placed)) {
            let own = component_name(f.component);
            let other = component_name(owner);
            let name = &body.name;
            return DocError::Invalid(format!(
                "{plain} in component '{own}'; the tool may overlap '{name}' in component '{other}' — activate that component or pick the body as a target"
            ));
        }
    }
    DocError::Invalid(plain.into())
}

/// [`apply_op`] with tools whose lineage is known: the bodies it makes or changes keep theirs.
fn apply_traced(doc: &Document, state: &mut ModelState, f: &Feature, tools: Vec<Traced>, op: Operation, targets: &[String]) -> Result<()> {
    apply_traced_with(doc, state, f, tools, op, targets, &[])
}

/// [`apply_traced`] with bodies that take part in the boolean besides the feature's targets:
/// the body a Join extrude runs to (#6424), which joins with the tool.
fn apply_traced_with(
    doc: &Document,
    state: &mut ModelState,
    f: &Feature,
    tools: Vec<Traced>,
    op: Operation,
    targets: &[String],
    extra: &[String],
) -> Result<()> {
    // Participants: the feature's own targets, else its explicit participants, else the bodies
    // of its component (and what it made itself): other components are left alone.
    let targets: &[String] = if targets.is_empty() { &f.participants } else { targets };
    let all = targets.len() == 1 && targets.first().is_some_and(|t| t == "*");
    let named = !all && !targets.is_empty();
    let mut target_idx: Vec<usize> = if all {
        (0..state.bodies.len()).filter(|i| state.bodies.get(*i).is_some_and(|b| !b.envelope)).collect()
    } else if targets.is_empty() {
        (0..state.bodies.len()).filter(|i| state.bodies.get(*i).is_some_and(|b| b.in_default_scope(f))).collect()
    } else {
        let mut v = Vec::new();
        for t in targets {
            let i = match named_target(doc, state, f, t) {
                // A target an earlier Combine has since joined into its own target is that
                // body now (#17807), as a Combine tool that is part of another body is.
                Err(DocError::Unknown(_)) if let Some(base) = joined_into(doc, state, f, t) => {
                    JOINED_TARGETS.with(|n| n.borrow_mut().push(format!("`{t}` is part of `{base}` now")));
                    let i = named_target(doc, state, f, base)?;
                    if v.contains(&i) {
                        continue;
                    }
                    i
                }
                r => r?,
            };
            v.push(i);
        }
        v
    };
    // A surface body is no solid to join a solid tool with: the union of the two fails in the
    // kernel. A join that names no target leaves the surface bodies alone, Thicken's own source
    // among them (#12326), and joins the solids; with none it makes a new body. A named surface
    // target is not dropped here, and a tool that is itself a surface keeps its targets.
    if op == Operation::Join && !named && tools.iter().all(|t| !t.body.is_mesh() && !t.body.is_surface()) {
        target_idx.retain(|&i| state.bodies.get(i).is_none_or(|b| !b.is_surface()));
    }
    // Bodies that take part besides the feature's own targets: the body a Join extrude runs to
    // joins with the tool (#6424), when it is in the feature's own component. A body of another
    // component (the shell a connector's boss runs up to) joins only when the feature names it:
    // an extent's end face never pulls a part across a component boundary.
    for name in extra {
        if let Some(i) = state.bodies.iter().position(|b| &b.name == name && b.component == f.component)
            && !target_idx.contains(&i)
        {
            target_idx.push(i);
        }
    }
    if op == Operation::NewBody || target_idx.is_empty() {
        if op == Operation::Cut || op == Operation::Intersect {
            return Err(empty_boolean_targets(doc, state, f, &tools, targets.is_empty()));
        }
        for (k, t) in tools.into_iter().enumerate() {
            let name = new_name(state, f, k);
            state.bodies.push(ModelBody::traced_new(name, t, f.id));
        }
        return Ok(());
    }
    // A surface body is no solid to cut or intersect, including automatic targets (#17587).
    // Automatic cuts with solid targets leave surfaces intact. Keep Thicken's sources too.
    // Surface-only cuts leave a surface alone when exact bounds prove every tool is clear of
    // it in its own frame. An intersect would delete even a distant surface.
    if matches!(op, Operation::Cut | Operation::Intersect) {
        let cuts_solids = op == Operation::Cut && !named && target_idx.iter().any(|&i| state.bodies.get(i).is_some_and(ModelBody::is_closed));
        let mut untouched = std::collections::BTreeSet::new();
        for &i in &target_idx {
            if let Some(b) = state.bodies.get(i).filter(|b| b.is_surface()) {
                // Surface targets never block an automatic solid cut or lose faces to it.
                // Thicken's source surfaces also stay intact for intersects (#17587).
                if cuts_solids || (!named && matches!(&f.kind, FeatureKind::Thicken { bodies, .. } if bodies.iter().any(|n| *n == b.name))) {
                    untouched.insert(i);
                    continue;
                }
                if op == Operation::Cut && !named && !tools.is_empty() {
                    let into = tool_frame(&b.name);
                    let mut apart = true;
                    for tool in &tools {
                        if !kernel::apart(&b.body, &tool_in(tool, into)?.body)? {
                            apart = false;
                            break;
                        }
                    }
                    if apart {
                        untouched.insert(i);
                        continue;
                    }
                }
                let verb = if op == Operation::Cut { "a cut" } else { "an intersect" };
                return Err(DocError::Invalid(format!(
                    "{} is a surface body, and {verb} needs a solid: cut it with Surface › Trim, or Thicken it first",
                    b.name
                )));
            }
        }
        target_idx.retain(|i| !untouched.contains(i));
    }
    // An intersect's tools act as one body. Unioned after the refusals above, so a union that
    // fails or runs out of time never hides them and is not paid for a refused feature.
    let tools = if op == Operation::Intersect { intersection_tool_union(tools)? } else { tools };
    match op {
        Operation::Join => {
            let joined = join(state, tools, &target_idx)?;
            if f.touching == crate::Touching::Separate {
                let mut kept = None;
                for name in joined {
                    kept = kept.max(separate_touching(state, &name)?);
                }
                if let Some(k) = kept {
                    let most = state.kept_separate.entry(f.id).or_insert(k);
                    *most = (*most).max(k);
                }
            }
        }
        Operation::Cut | Operation::Intersect => {
            let bop = if op == Operation::Cut { BoolOp::Cut } else { BoolOp::Intersect };
            let tools = if op == Operation::Cut { clear_tools_as_one(tools)? } else { tools };
            // Each target's pieces before the cut: a body joined from separate pieces stays one.
            let affected: std::collections::BTreeMap<String, usize> =
                target_idx.iter().filter_map(|i| state.bodies.get(*i)).map(|b| (b.name.clone(), b.body.lumps().map_or(1, |l| l.len()))).collect();
            let mut remove = Vec::new();
            // A cut hands a body its tools cannot reach back as it was, but only after a whole
            // boolean: in a design of many bodies that was most of a hole's time (#10225). A body
            // whose exact box lies clear of every tool's is left as it is. A body shown in another
            // frame, and a mesh body, are cut as before; an intersect removes the bodies it misses.
            // The exact box of the body is measured only when it can show a tool apart of it: a
            // tool inside the body is not apart from it, and measuring the box of a body with
            // many faces costs the kernel minutes (#18821: 347 s for the cup of the report,
            // against 30 s for the cut the box was measured for). Nor is a tool that meets the
            // box of the body's vertices, which lies inside its exact box.
            let reach: Option<Vec<Option<solidcraft_geom::Aabb3>>> =
                (op == Operation::Cut && !tools.iter().any(|t| t.body.is_mesh())).then(|| tools.iter().map(|t| more::exact_box(&t.body)).collect());
            for &ti in &target_idx {
                let Some(target) = state.bodies.get(ti) else { continue };
                // The tools meet the target where it is shown: in its frame (#3383).
                let into = tool_frame(&target.name);
                if let Some(reach) = &reach
                    && into.is_none()
                    && !reach.is_empty()
                    && !target.body.is_mesh()
                    // The box of the vertices first: it needs no mesh of the body.
                    && !more::tools_meet_vertices(target, reach)
                    && !more::tools_inside(target, reach)
                    && let Some(tb) = more::exact_box(&target.body)
                    && reach.iter().all(|tool| more::boxes_apart(&Some(tb), tool))
                {
                    continue;
                }
                #[cfg(test)]
                cost_tests::count(|c| c.cut += 1);
                let placed;
                let tools: &[Traced] = match into {
                    Some(into) => {
                        placed = tools.iter().map(|t| tool_in(t, Some(into))).collect::<Result<Vec<_>>>()?;
                        &placed
                    }
                    None => &tools,
                };
                let acc = target.cut(tools, bop)?;
                if op == Operation::Intersect && acc.as_ref().is_none_or(|b| b.body.face_count() == 0) {
                    // A tool that meets the body yet leaves nothing is a failed boolean, not an empty result.
                    let mut met = false;
                    if !named && !target.body.is_mesh() {
                        for t in tools {
                            if target.body.meets(&t.body)? {
                                met = true;
                                break;
                            }
                        }
                    }
                    if named || met {
                        return Err(empty_intersection(&[target.name.as_str()]));
                    }
                    remove.push(ti);
                    continue;
                }
                match acc {
                    Some(b) => {
                        if let Some(mb) = state.bodies.get_mut(ti) {
                            *mb = mb.remade(b);
                        }
                    }
                    None if op == Operation::Cut && !more::tools_may_cover(&target.body, tools) => return Err(more::failed_cut(&target.name)),
                    None => remove.push(ti),
                }
            }
            if op == Operation::Intersect && target_idx.iter().all(|i| remove.contains(i)) {
                let names: Vec<&str> = target_idx.iter().filter_map(|i| state.bodies.get(*i)).map(|b| b.name.as_str()).collect();
                return Err(empty_intersection(&names));
            }
            let mut i = 0;
            state.bodies.retain(|_| {
                let keep = !remove.contains(&i);
                i += 1;
                keep
            });
            let live: std::collections::BTreeSet<&str> = state.bodies.iter().map(|b| b.name.as_str()).collect();
            state.threads.retain(|t| live.contains(t.body.as_str()));
            // A sheet whose material the boolean took away goes with its body.
            state.surface_sheets.retain(|s| live.contains(s.body.as_str()));
            // A cut can leave a body in pieces: each piece becomes its own body ("Bar", "Bar (1)"),
            // numbered on past names already taken (a body cut in pieces before). Only a cut that
            // made more pieces than the body had splits it: the pieces of a join stay together.
            // Not yet: a cut that splits one piece of such a body still splits all of them; keeping
            // the untouched ones together needs each new piece traced to the piece it came from.
            let mut taken: std::collections::BTreeSet<String> = state.bodies.iter().map(|b| b.name.clone()).collect();
            // The pieces past the first belong to this feature, like a split's pieces (#10123):
            // `body.rename` stores their names in this feature's `body_names`, in this order, so a
            // renamed piece keeps its own name through a rebuild instead of landing on a sibling
            // of the body it was cut from.
            let mut extra = 0usize;
            let mut split: Vec<ModelBody> = Vec::new();
            for mb in std::mem::take(&mut state.bodies) {
                // Sheet metal keeps its body whole: what the cut left of the sheet is read back
                // onto it.
                let sheet = state.surface_sheets.iter().any(|s| s.body == mb.name);
                let Some(&had) = affected.get(&mb.name).filter(|_| !sheet) else {
                    split.push(mb);
                    continue;
                };
                let pieces = mb.body.lumps().unwrap_or_else(|_| vec![mb.body.clone()]);
                if pieces.len() <= had {
                    split.push(mb);
                    continue;
                }
                split.extend(named_pieces_of(&mb, pieces, &mut taken, f.id, &f.body_names, &mut extra));
            }
            state.bodies = split;
        }
        Operation::NewBody => {}
    }
    Ok(())
}

/// Join `tools` into the bodies at `target_idx`: each tool joins every target it meets into one
/// body, kept under the first one's name; a tool that meets none joins the first target as a
/// separate piece. A sheet metal body keeps its own identity: only the first body met may be
/// one. The names of the bodies the tools went into.
fn join(state: &mut ModelState, tools: Vec<Traced>, target_idx: &[usize]) -> Result<Vec<String>> {
    let mut live: Vec<String> = target_idx.iter().filter_map(|i| state.bodies.get(*i).map(|b| b.name.clone())).collect();
    // Each union keeps the lineage of what it joins, so the joined body's faces keep
    // their names (the tool's new faces are named by their roles).
    let union = |a: &Traced, b: &Traced| -> Result<Traced> {
        let u = crate::flush_join::join_bodies(&a.body, &b.body, "join produced nothing")?;
        Ok(Traced::step(u, &[a, b]))
    };
    // One target takes every tool, whichever it meets: the result is the union of all of
    // them in one kernel call, checked once.
    if let [keep] = live.as_slice()
        && tools.len() > 1
    {
        let Some(base) = state.body(keep).map(ModelBody::traced) else { return Ok(Vec::new()) };
        // The tools go into the kept body's frame, where it is shown (#3383).
        let into_keep = tool_frame(keep);
        let tools = tools.iter().map(|tool| tool_in(tool, into_keep)).collect::<Result<Vec<_>>>()?;
        let operands: Vec<&Traced> = std::iter::once(&base).chain(&tools).collect();
        // Bodies that are all apart need no intersection work: they are one body of several
        // pieces as they stand (#3455: joining 64 separate pegs took the kernel's union 4.5 s).
        let acc = match union_all_apart(&operands) {
            Some(acc) => acc,
            // The kernel's union pairs the tools with each other before the target. Fins whose
            // ends meet only inside a hub then meet in slivers it cannot resolve (#15913); into
            // the target one at a time, each tool's overlap with the others already lies inside.
            None => match union_all(&operands) {
                Ok(acc) => acc,
                Err(e) => tools.iter().try_fold(base.clone(), |acc, tool| union(&acc, tool)).map_err(|_| e)?,
            },
        };
        if let Some(mb) = state.bodies.iter_mut().find(|b| b.name == *keep) {
            *mb = mb.remade(acc);
        }
        return Ok(vec![keep.clone()]);
    }
    let mut joined = Vec::new();
    for tool in &tools {
        let mut met: Vec<String> = Vec::new();
        // Which bodies the tool meets picks the one it joins and those joined with it. A single
        // body takes the tool whether it meets it or not, so it is not asked: the test meshes
        // the body finely, and that took longer than the join itself on a part of a few hundred
        // faces.
        for name in live.iter().filter(|_| live.len() > 1) {
            let Some(mb) = state.body(name) else { continue };
            let sheet = state.surface_sheets.iter().any(|s| &s.body == name);
            if mb.body.is_mesh() || (!met.is_empty() && sheet) {
                continue;
            }
            #[cfg(test)]
            cost_tests::count(|c| c.contact_tests += 1);
            // Each body is met where it is shown: the tool goes into its frame (#3383).
            if mb.body.meets(&tool_in(tool, tool_frame(name))?.body)? {
                met.push(name.clone());
            }
        }
        let Some(keep) = met.first().or(live.first()).cloned() else { return Ok(joined) };
        let Some(base) = state.body(&keep).map(ModelBody::traced) else { continue };
        let into_keep = tool_frame(&keep);
        let mut acc = union(&base, &tool_in(tool, into_keep)?)?;
        for name in met.iter().skip(1) {
            if let Some(b) = state.body(name) {
                // A body placed differently from the one kept joins it where it is shown.
                let from = match tool_frame(name) {
                    Some(m) => crate::mat_inverse(&m),
                    None => Some(crate::IDENTITY),
                };
                let m = from.map(|from| crate::mat_mul(&into_keep.unwrap_or(crate::IDENTITY), &from)).filter(|m| !crate::is_identity(m));
                acc = union(&acc, &tool_in(&b.traced(), m)?)?;
            }
        }
        let absorbed: std::collections::BTreeSet<String> = met.into_iter().skip(1).collect();
        if let Some(mb) = state.bodies.iter_mut().find(|b| b.name == keep) {
            *mb = mb.remade(acc);
        }
        state.bodies.retain(|b| !absorbed.contains(&b.name));
        for t in state.threads.iter_mut().filter(|t| absorbed.contains(&t.body)) {
            t.body = keep.clone();
        }
        live.retain(|n| !absorbed.contains(n));
        if !joined.contains(&keep) {
            joined.push(keep);
        }
    }
    Ok(joined)
}

/// With `Touching::Separate`: the joined body `name` never keeps two pieces that touch only
/// along an edge or at a point, since they cannot be one solid. Its pieces that share a face are
/// one group. The groups that hold the material of the body before the feature (the first group
/// where that is not known) stay in it, each unless it touches one already kept; every other
/// group that touches another along an edge or at a point (as the record says, or on the exact
/// geometry where SMLib left the contact out) becomes a body of its own, named as a cut's pieces
/// are; groups that touch nothing stay in the joined body, which keeps its name. How
/// the pieces split off touched, when any were (a sheet metal or mesh body keeps its pieces).
fn separate_touching(state: &mut ModelState, name: &str) -> Result<Option<KeptSeparate>> {
    let Some(i) = state.bodies.iter().position(|b| b.name == name) else { return Ok(None) };
    let Some(mb) = state.bodies.get(i).filter(|b| !b.body.is_mesh() && !state.surface_sheets.iter().any(|s| s.body == name)) else { return Ok(None) };
    let n = mb.body.record().material_regions().len();
    if n < 2 {
        return Ok(None);
    }
    // Pieces sharing a face are one solid: each group is listed under its least piece.
    let contacts = mb.body.region_contacts()?;
    let mut root: Vec<usize> = (0..n).collect();
    let find = |root: &[usize], mut k: usize| {
        while let Some(&r) = root.get(k).filter(|&&r| r != k) {
            k = r;
        }
        k
    };
    for c in contacts.iter().filter(|c| c.contact == Contact::Face) {
        let [a, b] = c.regions.map(|k| find(&root, k));
        if let Some(slot) = root.get_mut(a.max(b)) {
            *slot = a.min(b);
        }
    }
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut group_of: Vec<usize> = Vec::with_capacity(n);
    let mut by_root: BTreeMap<usize, usize> = BTreeMap::new();
    for k in 0..n {
        let g = *by_root.entry(find(&root, k)).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        if let Some(list) = groups.get_mut(g) {
            list.push(k);
        }
        group_of.push(g);
    }
    // The groups each group touches along an edge or at a point (groups never share a face), and
    // the most any two of them share.
    let mut touches: Vec<Vec<usize>> = vec![Vec::new(); groups.len()];
    let mut most: Option<KeptSeparate> = None;
    for c in &contacts {
        let (Some(&a), Some(&b)) = (group_of.get(c.regions[0]), group_of.get(c.regions[1])) else { continue };
        if a == b {
            continue;
        }
        for (g, h) in [(a, b), (b, a)] {
            if let Some(list) = touches.get_mut(g).filter(|l| !l.contains(&h)) {
                list.push(h);
            }
        }
        most = most.max(Some(if c.contact == Contact::Vertex { KeptSeparate::AtPoint } else { KeptSeparate::AlongEdge }));
    }
    // Groups the record has apart may still touch where SMLib left the contact out of the
    // topology (a face tangent to another along a line).
    if groups.len() > 1 {
        for [a, b] in unrecorded_touches(&mb.body.parts(&groups)?, &touches)? {
            for (g, h) in [(a, b), (b, a)] {
                if let Some(list) = touches.get_mut(g) {
                    list.push(h);
                }
            }
            most = most.max(Some(KeptSeparate::Touching));
        }
    }
    let Some(most) = most else { return Ok(None) };
    // What stays in the joined body: the groups that touch nothing, and those of the body before
    // the feature, each unless it touches one already kept.
    let mut anchor = groups_from_body(mb, &groups).filter(|a| a.contains(&true)).unwrap_or_default();
    if anchor.is_empty() {
        anchor = (0..groups.len()).map(|g| g == 0).collect();
    }
    let mut stays: Vec<bool> = touches.iter().map(Vec::is_empty).collect();
    for g in 0..groups.len() {
        let free = touches.get(g).is_some_and(|t| !t.iter().any(|h| stays.get(*h) == Some(&true)));
        if free
            && anchor.get(g) == Some(&true)
            && let Some(slot) = stays.get_mut(g)
        {
            *slot = true;
        }
    }
    let mut kept: Vec<usize> = groups.iter().zip(&stays).filter(|(_, s)| **s).flat_map(|(g, _)| g.iter().copied()).collect();
    kept.sort_unstable();
    let parts: Vec<Vec<usize>> = std::iter::once(kept).chain(groups.into_iter().zip(&stays).filter(|(_, s)| !**s).map(|(g, _)| g)).collect();
    let pieces = mb.body.parts(&parts)?;
    let mut taken: std::collections::BTreeSet<String> = state.bodies.iter().map(|b| b.name.clone()).collect();
    let named = named_pieces(mb, pieces, &mut taken);
    state.bodies.splice(i..=i, named);
    Ok(Some(most))
}

/// The pairs of `pieces` (positions) that touch though `touches` (the pieces each touches, as
/// the record says) doesn't list them: their least distance on the exact faces and edges is
/// within the larger of their record tolerances. A distance only the meshes give is not trusted
/// to say they touch.
fn unrecorded_touches(pieces: &[Body], touches: &[Vec<usize>]) -> Result<Vec<[usize; 2]>> {
    let meshes = pieces
        .iter()
        .map(|p| {
            let chord = (p.size() * 1e-4).max(1e-6);
            Ok((p.mesh(&kernel::MeshTolerance::chord(chord))?, chord))
        })
        .collect::<Result<Vec<_>>>()?;
    let shapes: Vec<kernel::DistanceShape> = pieces.iter().zip(&meshes).map(|(p, (m, chord))| kernel::DistanceShape::new(p, m, *chord)).collect();
    let mut out = Vec::new();
    for (a, (sa, pa)) in shapes.iter().zip(pieces).enumerate() {
        for (b, (sb, pb)) in shapes.iter().zip(pieces).enumerate().skip(a + 1) {
            if touches.get(a).is_some_and(|t| t.contains(&b)) || !sa.bounds().meets(&sb.bounds()) {
                continue;
            }
            let tol = pa.record().tolerance.max(pb.record().tolerance);
            if kernel::body_distance(sa, sb, tol)?.is_some_and(|d| d.exact && d.distance <= tol) {
                out.push([a, b]);
            }
        }
    }
    Ok(out)
}

/// Which of `groups` (of positions in `mb`'s material regions) hold material of the body named
/// as `mb` before the feature: a face of theirs came from one of its faces. `None` where the
/// body's lineage is not known.
fn groups_from_body(mb: &ModelBody, groups: &[Vec<usize>]) -> Option<Vec<bool>> {
    let from = mb.lineage()?.faces_from_body(&mb.name);
    let record = mb.body.record();
    let regions = record.material_regions();
    let holds = |k: usize| {
        let shells = regions.get(k).and_then(|&r| record.regions.get(r)).map(|r| r.shells.as_slice()).unwrap_or_default();
        shells.iter().any(|&s| match record.shells.get(s as usize) {
            Some(solidcore_record::brep::Shell::Faces { faces, .. }) => faces.iter().any(|f| from.get(f.face as usize) == Some(&true)),
            _ => false,
        })
    };
    Some(groups.iter().map(|g| g.iter().any(|&k| holds(k))).collect())
}

/// The bodies the pieces of `mb` become: the first keeps its name, the others are numbered after
/// it ("Bar (1)") on past the names already `taken`. Each piece's faces are named after the
/// whole's, worked out once for the whole (so pieces of one face in different bodies are told
/// apart).
fn named_pieces(mb: &ModelBody, pieces: Vec<Body>, taken: &mut std::collections::BTreeSet<String>) -> Vec<ModelBody> {
    named_pieces_of(mb, pieces, taken, mb.feature, &[], &mut 0)
}

/// [`named_pieces`] for a feature that made the pieces past the first: they are `feature`'s, and
/// take the names stored for it from `stored`, from the place `extra` keeps (#10123), where one
/// is set and free.
fn named_pieces_of(
    mb: &ModelBody,
    pieces: Vec<Body>,
    taken: &mut std::collections::BTreeSet<String>,
    feature: u64,
    stored: &[String],
    extra: &mut usize,
) -> Vec<ModelBody> {
    let lineages = crate::lineage::Lineage::lumps(&mb.body, mb.lineage().as_deref(), &pieces);
    let mut out = Vec::with_capacity(pieces.len());
    for (k, (p, lineage)) in pieces.into_iter().zip(lineages).enumerate() {
        let (name, made_by) = if k == 0 {
            (mb.name.clone(), mb.feature)
        } else {
            let kept = stored.get(*extra).filter(|n| !n.trim().is_empty() && !taken.contains(*n)).cloned();
            *extra += 1;
            let name =
                kept.unwrap_or_else(|| (k..=k + taken.len()).map(|j| format!("{} ({j})", mb.name)).find(|n| !taken.contains(n)).unwrap_or_default());
            (name, feature)
        };
        taken.insert(name.clone());
        // Each piece stays in the whole's component until the feature ends (#14305).
        let mut piece = ModelBody::new(name, p, made_by).with_lineage(lineage);
        piece.component = mb.component;
        if k > 0 {
            piece.split_from = Some(mb.name.clone());
        }
        out.push(piece);
    }
    out
}

/// Distance that reaches through every target body from the plane `start` along the normal from
/// the sketch plane (the From Object level plus any start offset), on either side.
pub fn through_all_distance(st: &ModelState, plane: &Plane, start: f64, targets: &[String]) -> Result<f64> {
    let mut far: f64 = 0.0;
    let mut any = false;
    for b in st.bodies.iter().filter(|b| targets.is_empty() || targets.contains(&b.name)) {
        let bb = b.body.exact_bounds().map_err(|e| DocError::Invalid(e.to_string()))?;
        if bb.is_empty() {
            continue;
        }
        any = true;
        for i in 0..8 {
            let p = Vec3::new(
                if i & 1 == 0 { bb.min.x } else { bb.max.x },
                if i & 2 == 0 { bb.min.y } else { bb.max.y },
                if i & 4 == 0 { bb.min.z } else { bb.max.z },
            );
            far = far.max((plane.height(p) - start).abs());
        }
    }
    if !any {
        return Err(DocError::Invalid("through all needs a body to cut".into()));
    }
    Ok(far * 1.02 + 1.0)
}

/// The body a face-based feature applies to: by name, or the one whose surface is nearest the
/// first point.
fn body_at(st: &ModelState, body: &Option<crate::BodyName>, points: &[Vec3]) -> Result<usize> {
    if let Some(n) = body {
        return st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")));
    }
    let p = points.first().ok_or_else(|| DocError::Invalid("no faces selected".into()))?;
    st.bodies
        .iter()
        .enumerate()
        .map(|(i, b)| (i, box_dist(b, *p)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .ok_or_else(|| DocError::Invalid("there is no body".into()))
}

/// How far `p` is from body `b`'s exact box (0 inside it).
fn box_dist(b: &ModelBody, p: Vec3) -> f64 {
    #[cfg(test)]
    cost_tests::count(|c| c.tight_boxes += 1);
    match b.body.exact_bounds() {
        Ok(bb) if !bb.is_empty() => Vec3::new(p.x.clamp(bb.min.x, bb.max.x), p.y.clamp(bb.min.y, bb.max.y), p.z.clamp(bb.min.z, bb.max.z)).dist(p),
        _ => f64::INFINITY,
    }
}

/// Whether the whole line through `p` along `dir` (both ways) meets the box (slab test). A zero
/// component of `dir` keeps the line in its slab only when `p` is already within it.
fn line_meets_box(p: Vec3, dir: Vec3, b: &solidcraft_geom::Aabb3) -> bool {
    if b.is_empty() {
        return false;
    }
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for (o, d, min, max) in [(p.x, dir.x, b.min.x, b.max.x), (p.y, dir.y, b.min.y, b.max.y), (p.z, dir.z, b.min.z, b.max.z)] {
        if d == 0.0 || !d.is_finite() {
            if !(min..=max).contains(&o) {
                return false;
            }
            continue;
        }
        let (t0, t1) = ((min - o) / d, (max - o) / d);
        lo = lo.max(t0.min(t1));
        hi = hi.min(t0.max(t1));
    }
    lo <= hi
}

/// A hole stays on its face when the model changes: a position now inside a body moves back
/// against the drilling direction to the surface, one now above it moves down onto it. Only the
/// bodies in `scope` count (every body when it is empty, see [`hole_scope`]). Edit Hole seeds from
/// this point so the dialog's face pick matches where evaluation drills (#10689).
pub fn hole_on_surface(st: &ModelState, p: Vec3, dir: Vec3, scope: &[String]) -> Vec3 {
    hole_on_surface_in(st, p, dir, scope, true)
}

/// [`hole_on_surface`]; with `from_inside` false a position inside a body stays where it is (a
/// hole from a sketch on an explicit plane starts at that plane, #11538). Exact: the body says
/// whether the position is in it or on it, and the ray along the axis meets its faces' own surfaces
/// ([`solidcraft_kernel::Body::fire_ray`]). A display mesh's facets lie off a curved face by their
/// chord: a spot moved onto the facets of a bore (a hole drilled out from a tube's axis) started in
/// the air short of the bore, and the material between it and the bore was left (#20424).
fn hole_on_surface_in(st: &ModelState, p: Vec3, dir: Vec3, scope: &[String], from_inside: bool) -> Vec3 {
    let size = st.bodies.iter().map(|b| b.body.size()).fold(1.0, f64::max);
    let eps = size * 1e-6;
    let mut best: Option<Vec3> = None;
    for b in st.bodies.iter().filter(|b| in_hole_scope(scope, b)) {
        // The ray runs along the line through `p`: a body whose exact box that line misses has
        // nothing for it to meet (in an assembly of hundreds of parts, most of them).
        if b.body.exact_bounds().is_ok_and(|bb| !line_meets_box(p, dir, &bb.inflated(eps * 8.0))) {
            continue;
        }
        let inside = match b.body.classify_point(p) {
            // Already on the surface: the drill enters right here.
            Ok(kernel::PointClass::OnBoundary) => return p,
            Ok(kernel::PointClass::Inside) if !from_inside => return p,
            Ok(kernel::PointClass::Inside) => true,
            Ok(kernel::PointClass::Outside) => false,
            // A body the kernel cannot classify the spot against does not move it.
            Err(_) => continue,
        };
        // Inside, back against the drilling direction to where the drill enters; outside, on
        // along it to the face it meets.
        let hit = b.body.fire_ray(p, if inside { -dir } else { dir });
        if let Ok(Some(h)) = hit
            && best.is_none_or(|bq| h.point.dist(p) < bq.dist(p))
        {
            best = Some(h.point);
        }
    }
    best.unwrap_or(p)
}

/// The faces of the model near `p`, nearest first, before their surfaces are looked at: for
/// each, how far `p` is from it, its body, its index and the outward normal of its nearest mesh
/// facet. Only faces within `reach` of `p`, of the bodies in `scope` (every body when it is
/// empty). Ties keep body order, then face order.
fn faces_near(st: &ModelState, p: Vec3, reach: f64, scope: &[String]) -> Vec<NearFace> {
    let mut out: Vec<NearFace> = Vec::new();
    for (i, b) in st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh() && in_hole_scope(scope, b)) {
        let m = b.mesh();
        // No facet is nearer `p` than the mesh's box: one beyond `reach` has none to give.
        if !m.bounds().inflated(reach).contains(p) {
            continue;
        }
        // The nearest facet of each face of this body.
        let mut near: BTreeMap<u32, (f64, Vec3)> = BTreeMap::new();
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let Some([a, bb, c]) = m.tri(t) else { continue };
            let d = crate::appearance::closest_on_triangle(p, a, bb, c).dist(p);
            let Some(n) = (bb - a).cross(c - a).normalized() else { continue };
            if d <= reach && near.get(f).is_none_or(|(x, _)| d < *x) {
                near.insert(*f, (d, n));
            }
        }
        out.extend(near.into_iter().map(|(f, (d, n))| (d, i, f, n)));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// A face of [`faces_near`]: its distance from the point, its body, its index and the outward
/// normal of its nearest mesh facet.
type NearFace = (f64, usize, u32, Vec3);

/// One face of [`faces_near`] as [`faces_through`] gives it: the distance, the outward normal
/// (the surface's own normal for planes, cylinders, spheres, cones and tori, turned the way the
/// face looks out of its body; the nearest mesh facet's otherwise) and, for a plane, how far `p`
/// is from the plane itself. Classifying the surface is the costly part, so callers that stop at
/// the first face that answers them ask for no more than that.
fn face_through(st: &ModelState, p: Vec3, (d, body, f, facet): NearFace) -> (f64, Vec3, Option<f64>) {
    #[cfg(test)]
    cost_tests::count(|c| c.surfaces += 1);
    let exact = st.bodies.get(body).and_then(|b| kernel::face_surface(&b.body, f as usize));
    let (exact, off) = match exact {
        Some(kernel::FaceSurface::Plane { normal, point }) => (Some(*normal), Some((p - point).dot(normal).abs())),
        Some(kernel::FaceSurface::Cylinder { origin, axis, .. }) => {
            let v = p - origin;
            ((v - axis * v.dot(axis)).normalized(), None)
        }
        Some(kernel::FaceSurface::Sphere { center, .. }) => ((p - center).normalized(), None),
        // Square to the line through the apex, not the nearest facet's normal (#10698).
        Some(kernel::FaceSurface::Cone { apex, axis, half_angle }) => {
            let v = p - apex;
            let out = (v - axis * v.dot(axis)).normalized();
            (out.and_then(|u| (u * half_angle.cos() - axis * half_angle.sin()).normalized()), None)
        }
        // Away from the nearest point of the ring the tube goes round.
        Some(kernel::FaceSurface::Torus { center, axis, major, .. }) => {
            let v = p - center;
            let ring = (v - axis * v.dot(axis)).normalized().map(|u| center + u * major);
            (ring.and_then(|q| (p - q).normalized()), None)
        }
        _ => (None, None),
    };
    let n = match exact {
        Some(n) if n.dot(facet) < 0.0 => -n,
        Some(n) => n,
        None => facet,
    };
    (d, n, off)
}

/// The faces near `p` a hole placed there goes by (as [`faces_near`]): those of the bodies in
/// `scope`, the hole's own, when one of them passes within `on` of `p`, else those of every body.
/// Where a plate's face touches another body both faces pass through `p`, and a hole that drills
/// the plate goes by the plate's face, never the other body's (#12066). A hole placed on another
/// body's face, to drill through it into its own body behind, still goes by that face.
fn hole_faces_near(st: &ModelState, p: Vec3, reach: f64, on: f64, scope: &[String]) -> Vec<NearFace> {
    if !scope.is_empty() {
        let own = faces_near(st, p, reach, scope);
        if own.first().is_some_and(|c| c.0 <= on) {
            return own;
        }
    }
    faces_near(st, p, reach, &[])
}

/// The way a hole placed at `p` drills: into the material, against the outward normal of the
/// face `p` lies on. A point on no face (a planar face's centroid outside it, say) takes the
/// nearest planar face whose plane it lies in, else the face nearest it within its body's pick
/// tolerance. None when no face is near. Where the faces of several bodies pass through `p`, those
/// of the bodies in `scope` (the bodies the hole drills; empty for every body) decide.
pub fn hole_direction_at(st: &ModelState, p: Vec3, scope: &[String]) -> Option<Vec3> {
    let size = st.bodies.iter().map(|b| b.body.size()).fold(1.0, f64::max);
    // Every face of the model, nearest first; each surface is classified only when the rules
    // below reach it. Classifying all of them first took most of a hole's time in an assembly
    // of hundreds of parts, where the nearest face nearly always answers.
    let near = hole_faces_near(st, p, f64::INFINITY, size * 1e-4, scope);
    let mut faces = near.iter().map(|c| face_through(st, p, *c));
    let first = faces.next();
    if let Some((d, n, _)) = first
        && d <= size * 1e-4
    {
        return Some(-n);
    }
    if let Some((_, n, _)) = first.into_iter().chain(faces).find(|(_, _, off)| off.is_some_and(|o| o <= size * 1e-4)) {
        return Some(-n);
    }
    let reach = st.bodies.iter().map(crate::appearance::pick_tolerance).fold(0.0, f64::max);
    first.filter(|(d, _, _)| *d <= reach).map(|(_, n, _)| -n)
}

/// A hole placed on a face drills into the material: when its direction leaves every face it
/// starts on (as designs saved before format 18 can say: their direction was taken from any face
/// whose plane passed near the point, such as the other side of a thin plate or a face of another
/// body at the same height), it is mirrored in the face it starts on, so it goes in at the same
/// angle. A direction that enters a face there, or a spot on no face, is kept. Where the faces of
/// the hole's own bodies (`scope`, see [`hole_drill_scope`]) pass through the spot, only they
/// count: a hole that names the plate it drills, saved with the direction into a body touching
/// the plate there, drills the plate (#12066).
fn hole_dir_into_face(st: &ModelState, p: Vec3, dir: Vec3, scope: &[String]) -> Vec3 {
    let size = st.bodies.iter().map(|b| b.body.size()).fold(1.0, f64::max);
    let faces: Vec<_> = hole_faces_near(st, p, size * 1e-5, size * 1e-5, scope).into_iter().map(|c| face_through(st, p, c)).collect();
    if faces.is_empty() || faces.iter().any(|(_, n, _)| n.dot(dir) < -1e-9) {
        return dir;
    }
    match faces.iter().map(|(_, n, _)| *n).max_by(|a, b| a.dot(dir).total_cmp(&b.dot(dir))) {
        Some(n) if n.dot(dir) > 1e-9 => (dir - n * (2.0 * n.dot(dir))).normalized().unwrap_or(dir),
        _ => dir,
    }
}

/// Where a picked edge sat in its body: its direction (zero for curved edges) and the body's
/// bounding box at the time.
#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
pub struct EdgeRef {
    pub dir: Vec3,
    pub min: Vec3,
    pub max: Vec3,
}

fn edge_dir(e: &kernel::EdgeInfo) -> Vec3 {
    let (Some(a), Some(b)) = (e.points.first(), e.points.last()) else { return Vec3::ZERO };
    let straight = e.points.iter().all(|p| p.dist_to_segment(*a, *b) < 1e-6 * (1.0 + e.length));
    if straight && a.dist(*b) > 1e-9 { (*b - *a).normalized().unwrap_or(Vec3::ZERO) } else { Vec3::ZERO }
}

/// The body each edge picked at `pts` lies on (the state before a blend): the named body, else,
/// for each point on its own, the body with an edge nearest it. A blend over the edges of
/// several bodies remembers every edge on the body it was picked on, never on the first edge's.
fn picked_bodies(st: &ModelState, body: &Option<crate::BodyName>, pts: &[Vec3]) -> Option<Vec<usize>> {
    if body.is_some() || st.bodies.iter().filter(|b| !b.body.is_mesh()).count() < 2 {
        blend_target(st, body, pts).ok().map(|ti| vec![ti; pts.len()])
    } else {
        pts.iter().map(|p| blend_target(st, &None, std::slice::from_ref(p)).ok()).collect()
    }
}

/// References for edges picked at `pts` on the bodies a blend would go on (the state before it).
pub fn edge_refs(st: &ModelState, body: &Option<crate::BodyName>, pts: &[Vec3]) -> Vec<EdgeRef> {
    let Some(on) = picked_bodies(st, body, pts) else { return Vec::new() };
    let mut seen: BTreeMap<usize, Option<(Vec<kernel::EdgeInfo>, solidcraft_geom::Aabb3)>> = BTreeMap::new();
    let mut out = Vec::with_capacity(pts.len());
    for (p, ti) in pts.iter().zip(on) {
        let found = seen.entry(ti).or_insert_with(|| {
            let mb = st.bodies.get(ti)?;
            let size = mb.body.size();
            Some((mb.body.edges((size * 1e-3).max(1e-3)).ok()?, mb.mesh().bounds()))
        });
        let Some((edges, bb)) = found else { return Vec::new() };
        let e = edges.iter().min_by(|x, y| {
            let d = |e: &kernel::EdgeInfo| e.points.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
            d(x).total_cmp(&d(y))
        });
        out.push(EdgeRef { dir: e.map(edge_dir).unwrap_or(Vec3::ZERO), min: bb.min, max: bb.max });
    }
    out
}

/// Persistent names of the edges picked at `pts`, each on the body it lies on (see
/// [`picked_bodies`]): of the edge a blend takes at each pick (at a vertex where a seam meets a
/// rim, the rim), else of the nearest edge.
pub fn edge_names_for(st: &ModelState, body: &Option<crate::BodyName>, pts: &[Vec3]) -> Vec<String> {
    let Some(on) = picked_bodies(st, body, pts) else { return Vec::new() };
    let mut out = vec![String::new(); pts.len()];
    let mut done: Vec<usize> = Vec::new();
    for &ti in &on {
        if done.contains(&ti) {
            continue;
        }
        done.push(ti);
        let Some(b) = st.bodies.get(ti) else { continue };
        // The body's names are worked out once for all its picks.
        let idx: Vec<usize> = on.iter().enumerate().filter(|(_, t)| **t == ti).map(|(i, _)| i).collect();
        let at: Vec<Vec3> = idx.iter().filter_map(|i| pts.get(*i).copied()).collect();
        let names = match kernel::blend_edges(&b.body, &at) {
            Ok(edges) => {
                let all = crate::naming::edge_names(b);
                edges.iter().map(|e| all.get(*e).cloned().unwrap_or_default()).collect()
            }
            Err(_) => crate::naming::edge_names_at(b, &at),
        };
        for (i, n) in idx.into_iter().zip(names) {
            if let Some(slot) = out.get_mut(i) {
                *slot = n;
            }
        }
    }
    out
}

/// The stored pick sits on the axis of a round, not on an edge. The nearest edge within a
/// quarter of the body is that round's tangency seam, which is a different edge (#775).
fn pick_lies_on_a_round_axis(body: &Body, p: Vec3) -> bool {
    let Some(cyl) = kernel::cylinder_face_at(body, p) else { return false };
    let Some(axis) = cyl.axis.normalized() else { return false };
    let radial = p - cyl.axis_point;
    let dist = (radial - axis * radial.dot(axis)).len();
    if !(cyl.radius > 1e-6 && dist < cyl.radius * 0.35) {
        return false;
    }
    let tight = (body.size() * 2e-3).max(1e-3);
    // A pick on the wall itself is the seam. Only an axial pick is the vanished corner.
    match body.nearest_edge(p, cyl.radius * 2.0) {
        Ok(Some((_, d))) => d > tight,
        _ => true,
    }
}

/// Edge reference points re-found on the body: points on an edge stay; a point that moved off
/// (an upstream edit) is carried with the body (its place in the body's bounding box then and
/// now) to an edge running the same way; failing that it goes to the nearest edge with a
/// warning; points with no edge near are dropped with a warning.
fn resolve_edges(b: &Body, pts: &[Vec3], refs: &[EdgeRef], warning: &mut Option<String>) -> Result<Vec<Vec3>> {
    let size = b.size();
    let edges = b.edges((size * 1e-3).max(1e-3))?;
    let tight = (size * 2e-3).max(1e-3);
    let dist = |p: Vec3, e: &kernel::EdgeInfo| e.points.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
    let now = b.mesh(&solidcraft_kernel::MeshTolerance::chord((size * 1e-2).max(1e-2))).map(|m| m.bounds()).ok();
    let (mut out, mut moved, mut lost) = (Vec::new(), 0, 0);
    for (i, p) in pts.iter().enumerate() {
        let on = edges.iter().any(|e| dist(*p, e) <= tight);
        if !on && let (Some(r), Some(nb)) = (refs.get(i), now.as_ref()) {
            // The same place in the body as it is now.
            let map = |v: f64, a0: f64, a1: f64, b0: f64, b1: f64| {
                if (a1 - a0).abs() > 1e-9 { b0 + (v - a0) * (b1 - b0) / (a1 - a0) } else { v + (b0 - a0) }
            };
            let q = Vec3::new(
                map(p.x, r.min.x, r.max.x, nb.min.x, nb.max.x),
                map(p.y, r.min.y, r.max.y, nb.min.y, nb.max.y),
                map(p.z, r.min.z, r.max.z, nb.min.z, nb.max.z),
            );
            let fits = |e: &kernel::EdgeInfo| {
                let d = edge_dir(e);
                dist(q, e) <= tight && (r.dir.len() < 0.5 || d.cross(r.dir).len() < 1e-3)
            };
            if edges.iter().any(fits) {
                out.push(q);
                continue;
            }
        }
        let best = edges
            .iter()
            .map(|e| (e.points.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min), e.mid))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((d, _)) if d <= tight => out.push(*p),
            Some((d, mid)) if d <= size * 0.25 => {
                out.push(mid);
                moved += 1;
            }
            _ => lost += 1,
        }
    }
    if out.is_empty() {
        return Err(DocError::Invalid("none of the selected edges exist any more".into()));
    }
    if moved + lost > 0 {
        let mut w = Vec::new();
        if moved > 0 {
            w.push(format!("{moved} edge reference(s) re-found on the nearest edge"));
        }
        if lost > 0 {
            w.push(format!("{lost} edge reference(s) no longer found and skipped"));
        }
        *warning = Some(w.join("; "));
    }
    Ok(out)
}

/// The body a fillet/chamfer applies to: by name, or the one closest to the first edge point.
pub fn blend_target(state: &ModelState, body: &Option<crate::BodyName>, edges: &[Vec3]) -> Result<usize> {
    if let Some(n) = body {
        return state.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")));
    }
    let p = edges.first().ok_or_else(|| DocError::Invalid("no edges selected".into()))?;
    let mut best: Option<(usize, f64)> = None;
    for (i, b) in state.bodies.iter().enumerate() {
        let tol = (b.body.size() * 2e-3).max(1e-3);
        if let Ok(Some((_, d))) = b.body.nearest_edge(*p, tol)
            && best.is_none_or(|(_, bd)| d < bd)
        {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i).ok_or_else(|| {
        if state.bodies.iter().any(|b| b.body.is_mesh()) {
            DocError::Invalid("there is no solid body to modify (mesh bodies have no edges)".into())
        } else {
            DocError::Invalid("there is no body to modify".into())
        }
    })
}

/// Whether `p` lies in the bounding box a reference recorded (with a little slack): an edge
/// picked on a body lies in that body's box.
fn in_ref_box(r: &EdgeRef, p: Vec3) -> bool {
    let slack = ((r.max - r.min).len() * 1e-2).max(1e-3);
    (r.min.x - slack..=r.max.x + slack).contains(&p.x)
        && (r.min.y - slack..=r.max.y + slack).contains(&p.y)
        && (r.min.z - slack..=r.max.z + slack).contains(&p.z)
}

/// The stored name of a blend's edge `i` (picked at `p`), when it can be trusted. Builds before
/// blends over several bodies were split by body named every edge on the first edge's body: the
/// name of a pick on another body is that body's nearest edge, and the reference's box (that
/// body's) does not hold the pick. Such a name (and its reference) is not used.
fn trusted_edge_name(f: &Feature, i: usize, p: Vec3) -> Option<&str> {
    let n = f.edge_names.get(i).filter(|n| !n.is_empty())?;
    match f.edge_refs.get(i) {
        Some(r) if !in_ref_box(r, p) => None,
        _ => Some(n.as_str()),
    }
}

/// The body (index) an edge name finds an edge on, when one does: the one where the edge still
/// runs through the pick `p`, else the nearest.
fn named_body(st: &ModelState, name: &str, p: Vec3) -> Option<usize> {
    // Names start with the feature that made the face: only bodies with such a face can hold it.
    let bare = crate::naming::legacy_name(name).unwrap_or(name);
    let token = bare.find(':').map(|k| &bare[..=k]);
    let mut best: Option<(usize, (bool, f64))> = None;
    for (i, b) in st.bodies.iter().enumerate() {
        if b.body.is_mesh() || token.is_some_and(|t| !crate::naming::face_names(b).iter().any(|n| n.contains(t))) {
            continue;
        }
        let Ok(hit) = crate::name_lookup::locate_projected_edge(b, name, p) else { continue };
        let score = (hit.on_point, -hit.point.dist(p));
        if best.as_ref().is_none_or(|(_, s)| score.partial_cmp(s).is_some_and(|o| o.is_gt())) {
            best = Some((i, score));
        }
    }
    best.map(|(i, _)| i)
}

/// The body (index) an edge picked at `p` lies on: the body with an edge through it.
fn body_on_edge(st: &ModelState, p: Vec3) -> Option<usize> {
    st.bodies.iter().enumerate().filter_map(|(i, b)| Some((i, edge_through(b, p)?))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(i, _)| i)
}

/// The body (index) an edge picked at `p` without a name lies on, as [`body_on_edge`], and the note
/// when `p` lies on edges of other bodies too: every body with an edge through `p` about as near
/// as the nearest one, the window the `solid.fillet` command refuses such a pick with (#20517).
fn body_on_edge_noted(st: &ModelState, p: Vec3) -> (Option<usize>, Option<String>) {
    let near: Vec<(usize, f64)> = st.bodies.iter().enumerate().filter_map(|(i, b)| Some((i, edge_through(b, p)?))).collect();
    let Some(&(chosen, d_min)) = near.iter().min_by(|a, b| a.1.total_cmp(&b.1)) else { return (None, None) };
    let mut names: Vec<&str> =
        near.iter().filter(|(_, d)| *d <= 2.0 * d_min + 1e-3).filter_map(|(i, _)| Some(st.bodies.get(*i)?.name.as_str())).collect();
    let first = st.bodies.get(chosen).map(|b| b.name.as_str()).unwrap_or_default();
    names.sort_by_key(|n| *n != first);
    let note = match names.split_last() {
        Some((last, others)) if !others.is_empty() => Some(format!(
            "[{}, {}, {}] lies on edges of {} bodies, {} and {last}: `{first}` is blended; name the one to blend with `body`",
            p.x,
            p.y,
            p.z,
            names.len(),
            others.join(", ")
        )),
        _ => None,
    };
    (Some(chosen), note)
}

/// How far `p` is from the edge of `b` that runs through it, when one does.
fn edge_through(b: &ModelBody, p: Vec3) -> Option<f64> {
    if b.body.is_mesh() {
        return None;
    }
    let size = b.body.size();
    let (_, d) = b.body.nearest_edge(p, (size * 1e-3).max(1e-3)).ok()??;
    (d <= (size * 2e-3).max(1e-3)).then_some(d)
}

/// The body (index) each edge of a blend is on: the named body; else, for each edge, the body
/// its stored name finds it on. A named edge that is gone stays on the bodies the feature's other
/// edges were found on; an unnamed one goes to the body the pick lies on an edge of; failing
/// both, the body nearest the first pick (as before blends were split by body). A named edge is
/// never moved onto another body because that body's edge is nearer. The notes say which
/// unnamed picks lie on edges of several bodies: the `solid.fillet` and `solid.chamfer` commands
/// refuse such a pick, so a design file that has one blends the nearest body, but says so (#20517).
fn blend_bodies(st: &ModelState, f: &Feature, body: &Option<crate::BodyName>, pts: &[Vec3]) -> Result<(Vec<usize>, Vec<String>)> {
    // Named, or only one body to blend: no lookup.
    if body.is_some() || st.bodies.iter().filter(|b| !b.body.is_mesh()).count() < 2 {
        let ti = blend_target(st, body, pts)?;
        return Ok((vec![ti; pts.len()], Vec::new()));
    }
    let names: Vec<Option<&str>> = pts.iter().enumerate().map(|(i, p)| trusted_edge_name(f, i, *p)).collect();
    let by_name: Vec<Option<usize>> = names.iter().zip(pts).map(|(n, p)| n.and_then(|n| named_body(st, n, *p))).collect();
    let mut named: Vec<usize> = by_name.iter().flatten().copied().collect();
    named.sort_unstable();
    named.dedup();
    // Of the bodies the feature's other edges were found on, the one with an edge nearest `p`.
    let nearest_named = |p: Vec3| {
        named
            .iter()
            .filter_map(|i| {
                let b = st.bodies.get(*i)?;
                let tol = (b.body.size() * 2e-3).max(1e-3);
                Some((*i, b.body.nearest_edge(p, tol).ok()??.1))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    let mut fallback = None;
    let mut out = Vec::with_capacity(pts.len());
    let mut notes = Vec::new();
    for ((b, n), p) in by_name.into_iter().zip(&names).zip(pts) {
        let found = match (b, n) {
            (Some(ti), _) => Some(ti),
            // A named edge that is gone stays with the feature's other bodies: another body's
            // edge where it was picked is not it.
            (None, Some(_)) => nearest_named(*p).or_else(|| body_on_edge(st, *p)),
            (None, None) => {
                let (on, note) = body_on_edge_noted(st, *p);
                notes.extend(note);
                on.or_else(|| nearest_named(*p))
            }
        };
        let ti = match (found, fallback) {
            (Some(ti), _) => ti,
            (None, Some(ti)) => ti,
            (None, None) => *fallback.insert(blend_target(st, &None, pts)?),
        };
        out.push(ti);
    }
    Ok((out, notes))
}

/// The edges `idx` of blend `f` found on body `mb`: by name, where the edge was stored with one.
/// A name that no longer finds exactly its edge fails the blend, asking for the edge to be picked
/// again; so does a name a build stored for the wrong body (see `trusted_edge_name`).
fn blend_edges_on(mb: &ModelBody, f: &Feature, edges: &[crate::Position], idx: &[usize], warning: &mut Option<String>) -> Result<Vec<Vec3>> {
    let pick = |i: usize| edges.get(i).map(|p| p.0).unwrap_or(Vec3::ZERO);
    if !f.edge_names.iter().any(|n| !n.is_empty()) {
        let pts: Vec<Vec3> = idx.iter().map(|i| pick(*i)).collect();
        let refs: Vec<EdgeRef> = idx.iter().map_while(|i| f.edge_refs.get(*i).cloned()).collect();
        return resolve_edges(&mb.body, &pts, &refs, warning);
    }
    let (mut out, mut notes) = (Vec::new(), Vec::new());
    for &i in idx {
        let p = pick(i);
        let Some(stored) = f.edge_names.get(i).filter(|n| !n.is_empty()) else {
            // An edge picked without a name: by its point. A pick on the axis of a round must not
            // fall through to the tangency seam (#775).
            let mut w = None;
            let refs: Vec<EdgeRef> = f.edge_refs.get(i).cloned().into_iter().collect();
            let skipped = "edge `?` no longer exists and was skipped";
            if pick_lies_on_a_round_axis(&mb.body, p) {
                notes.push(skipped.into());
            } else if let Ok(q) = resolve_edges(&mb.body, &[p], &refs, &mut w) {
                out.extend(q);
                notes.push("an edge reference without a name was found by its point".into());
            } else {
                notes.push(skipped.into());
            }
            continue;
        };
        let hit = trusted_edge_name(f, i, p)
            .and_then(|n| crate::name_lookup::locate_projected_edge(mb, n, p).ok())
            .ok_or_else(|| DocError::reselect(stored.as_str()))?;
        if let Some(note) = hit.note(stored) {
            notes.push(note);
        }
        // The body changed since the pick: the pick may now sit at an end of the edge.
        let moved = f.edge_refs.get(i).is_some_and(|r| {
            let now = mb.mesh().bounds();
            let tight = (mb.body.size() * 2e-3).max(1e-3);
            now.min.dist(r.min) > tight || now.max.dist(r.max) > tight
        });
        out.push(if moved { off_the_ends(mb, hit.edge, hit.point) } else { hit.point });
    }
    if out.is_empty() {
        let why = if notes.is_empty() { String::new() } else { format!(" ({})", notes.join("; ")) };
        return Err(DocError::Invalid(format!("none of the selected edges exist any more{why}")));
    }
    if !notes.is_empty() {
        *warning = Some(notes.join("; "));
    }
    Ok(out)
}

/// A point on edge `edge` of a body for the kernel to find it by: `p`, unless it lies at an end
/// of the edge (an upstream edit moved the edge along it), where the edges meeting there are as
/// near: then the edge's middle.
fn off_the_ends(mb: &ModelBody, edge: usize, p: Vec3) -> Vec3 {
    let m = mb.mesh();
    let Some(e) = m.edges.get(edge) else { return p };
    let tight = (mb.body.size() * 2e-3).max(1e-3);
    let (Some(a), Some(b)) = (e.first(), e.last()) else { return p };
    if p.dist(*a) > tight && p.dist(*b) > tight {
        return p;
    }
    let total: f64 = e.windows(2).map(|w| w[0].dist(w[1])).sum();
    let mut acc = 0.0;
    for w in e.windows(2) {
        let l = w[0].dist(w[1]);
        if acc + l >= total / 2.0 {
            let t = if l > 0.0 { (total / 2.0 - acc) / l } else { 0.0 };
            return w[0] + (w[1] - w[0]) * t;
        }
        acc += l;
    }
    p
}

/// The edges a tapered fillet tapered (a point on each, with its taper).
type Tapered = Vec<(Vec3, kernel::Taper)>;

/// A blend's refusal when the feature blended the picked edges' tangent chains too (tangent
/// propagation, the default), saying so and how to blend the picked edges alone.
fn with_propagation_hint(e: DocError) -> DocError {
    const HINT: &str = " (tangent propagation blended the picked edges' tangent chain with them; with it off, tangent_chain: false, only the picked edges are blended)";
    match e {
        DocError::Kernel(kernel::KernelError::TooLarge(m)) => DocError::Kernel(kernel::KernelError::TooLarge(m + HINT)),
        DocError::Kernel(kernel::KernelError::Invalid(m)) => DocError::Kernel(kernel::KernelError::Invalid(m + HINT)),
        DocError::Kernel(kernel::KernelError::Failed(m)) => DocError::Kernel(kernel::KernelError::Failed(m + HINT)),
        DocError::Invalid(m) => DocError::Invalid(m + HINT),
        e => e,
    }
}

/// How many points along each edge [`left_sharp_note`] checks.
const LEFT_SHARP_SAMPLES: usize = 7;

/// Points along each edge that `picks` run on into tangentially beyond the picked edges
/// themselves ([`kernel::tangent_chain_beyond_points`]), on `body` before the blend: the edges a
/// blend with tangent propagation would take with them, [`LEFT_SHARP_SAMPLES`] points each.
/// The kernel's error when it cannot give them. A chain it cannot walk (a pick that finds no
/// edge, a smoothness test it cannot decide) fails the blend too, which walks the same chain
/// first. Points along a chain edge it cannot work out (an edge without a curve, a curve it will
/// not evaluate, a point that is not finite) are never asked for by the blend, which can then
/// succeed: [`left_sharp_note`] says the check could not run, rather than nothing, which would
/// read as "no edge was left sharp" (#19246).
fn chain_beyond_picks(body: &Body, picks: &[Vec3]) -> kernel::Result<Vec<Vec<Vec3>>> {
    kernel::tangent_chain_beyond_points(body, picks, LEFT_SHARP_SAMPLES)
}

/// The note for a blend that did not follow tangent chains, when its `picked` edges run on
/// smoothly into edges it left sharp, such as one arc of a rim the body stores as several. An
/// edge of `beyond` ([`chain_beyond_picks`]) is left sharp when any of its points still lies on
/// the boundary of the result `blended`, decided exactly. A blended edge's points are no longer
/// on it (a round or a bevel cut them away or filled them in), so the pieces a seam splits a rim
/// into, which SMLib blends with any one of them, are not counted. A round that runs on past the
/// end of its edge takes part of the next edge and leaves the rest sharp, so the middle alone
/// does not tell. Without the note the result looks right: the volume changes a little and
/// nothing says the rest of the visible edge stayed sharp. The note does not promise a way to
/// blend those edges: a separate feature or tangent propagation may fail where they meet. When
/// the check cannot run (`beyond` is the kernel's error, or the result's points cannot be
/// classified) the note says so with the kernel's reason; the blend itself stands.
fn left_sharp_note(blended: &Body, beyond: &kernel::Result<Vec<Vec<Vec3>>>, picked: usize, kind: &FeatureKind) -> Option<String> {
    let (feature, why) = match kind {
        FeatureKind::Chamfer { .. } => ("chamfer", "tangent_chain: false chamfers only the picked edges"),
        FeatureKind::Fillet { style: crate::FilletStyle::Variable { .. } | crate::FilletStyle::Spline { .. }, .. } => {
            ("fillet", "a variable or spline round runs along its picked edge only")
        }
        _ => ("fillet", "tangent_chain: false rounds only the picked edges"),
    };
    let from = if picked == 1 { "the picked edge runs" } else { "the picked edges run" };
    let unchecked = |e: &kernel::KernelError| format!("could not check whether {from} on smoothly into edges this {feature} left sharp ({e})");
    let beyond = match beyond {
        Ok(beyond) => beyond,
        Err(e) => return Some(unchecked(e)),
    };
    let points: Vec<Vec3> = beyond.iter().flatten().copied().collect();
    if points.is_empty() {
        return None;
    }
    let classes = match blended.classify_points(&points) {
        Ok(classes) => classes,
        Err(e) => return Some(unchecked(&e)),
    };
    let mut start = 0;
    let n = beyond
        .iter()
        .filter(|edge| {
            let end = start + edge.len();
            let points = classes.get(start..end).unwrap_or_default();
            start = end;
            points.iter().any(|c| matches!(c, kernel::PointClass::OnBoundary))
        })
        .count();
    if n == 0 {
        return None;
    }
    let (edges, are, them) = if n == 1 { ("1 edge".to_string(), "is", "it") } else { (format!("{n} edges"), "are", "them") };
    Some(format!(
        "{edges} that {from} on into smoothly {are} still sharp because {why} (to blend {them} too, pick {them} in another {feature}, which may fail where the edges meet)"
    ))
}

/// Body `mb` with blend `f` applied at `edges` (radius or distance `r`), the radii a fitted fillet
/// used (when it reduced any) and the edges it tapered.
fn blend_body(
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    mb: &ModelBody,
    edges: &[Vec3],
    r: f64,
    each: Option<&[f64]>,
    warning: &mut Option<String>,
) -> Result<(Traced, Option<Vec<f64>>, Tapered)> {
    // Each edge at its own radius, in one blend (checked by the caller: constant, exact, rolling,
    // mitred, the picked edges alone).
    if let (FeatureKind::Fillet { turn, .. }, Some(each)) = (&f.kind, each) {
        let nb = kernel::fillet_each(&mb.body, edges, each, turn.kernel())?;
        return Ok((Traced::step(nb.with_preserved_planar_sources(&mb.body)?, &[&mb.traced()]), None, Vec::new()));
    }
    let (nb, from, used, tapered) = blend_raw(vals, f, mb, edges, r, warning)?;
    // Keeping the input's planar certificates may join faces; their history stays over the same
    // operand.
    Ok((Traced::step(nb.with_preserved_planar_sources(&mb.body)?, &[&from]), used, tapered))
}

/// The blended body of [`blend_body`], with the stage its last blend started from (the input,
/// or the body after the edges before the last for a blend made one edge at a time), the radii a
/// fitted fillet used and the edges it tapered. Every type, fit, corner and setback of `f` is
/// built here, so a pattern's or a mirror's copy of a blend is the same blend (#16912).
fn blend_raw(
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    mb: &ModelBody,
    edges: &[Vec3],
    r: f64,
    warning: &mut Option<String>,
) -> Result<(kernel::Body, Traced, Option<Vec<f64>>, Tapered)> {
    let input = mb.traced();
    let (mut used, mut tapered) = (None, Vec::new());
    if let FeatureKind::Fillet { turn: crate::FilletTurn::Swept, style, fit, corner, setbacks, .. } = &f.kind
        && (!style.is_constant() || *fit == crate::FilletFit::Taper || !corner.is_rolling() || !setbacks.is_empty())
    {
        return Err(DocError::Invalid(crate::SWEPT_TAKES.into()));
    }
    // The blended body, and the lineage of what the last blend started from.
    let (nb, from) = match &f.kind {
        // A curvature-continuous fillet's corners are G2 patches, whatever `corner` says: SMLib's
        // own corner patch is only tangent to its rounds.
        FeatureKind::Fillet { style: crate::FilletStyle::Curvature, setbacks, .. } => {
            let mut corners = Vec::with_capacity(setbacks.len());
            for c in setbacks {
                corners.push((c.vertex.0, val(vals, &c.distance, Kind::Length)?));
            }
            (kernel::fillet_g2_with_g2_corners(&mb.body, edges, r, &corners)?, input)
        }
        FeatureKind::Fillet { corner, setbacks, .. } if matches!(corner, crate::FilletCorner::G2) || !setbacks.is_empty() => {
            // Each setback: its corner's vertex and its distance.
            let mut corners = Vec::with_capacity(setbacks.len());
            for c in setbacks {
                corners.push((c.vertex.0, val(vals, &c.distance, Kind::Length)?));
            }
            match corner {
                crate::FilletCorner::G2 => (kernel::fillet_g2_corners(&mb.body, edges, r, &corners)?, input),
                crate::FilletCorner::Rolling => (kernel::fillet_setback(&mb.body, edges, r, &corners)?, input),
            }
        }
        FeatureKind::Fillet { style: crate::FilletStyle::Chord, .. } => (kernel::fillet_chord(&mb.body, edges, r)?, input),
        FeatureKind::Fillet { style: crate::FilletStyle::Variable { radius2, start, mid, smooth }, .. } => {
            let r2 = val(vals, radius2, Kind::Length)?;
            let mut pts = vec![(0.0, r), (1.0, r2)];
            for (t, e) in mid {
                pts.push((*t, val(vals, e, Kind::Length)?));
            }
            one_by_one(input, edges, |b, p| kernel::fillet_variable_points(b, p, start.0, &pts, *smooth))?
        }
        FeatureKind::Fillet { style: crate::FilletStyle::Spline { start, knots, radii }, .. } => {
            let mut values = vec![r];
            for e in radii {
                values.push(val(vals, e, Kind::Length)?);
            }
            let law = crate::FilletStyle::spline(knots, values);
            one_by_one(input, edges, |b, p| kernel::fillet_variable_spline(b, p, start.0, &law))?
        }
        FeatureKind::Fillet { fit: crate::FilletFit::Taper, .. } => {
            let (b, tapers) = kernel::fillet_tapered(&mb.body, edges, r)?;
            if let Some(note) = tapered_note(&tapers) {
                append_warning(warning, note);
            }
            tapered = edges.iter().copied().zip(tapers).filter_map(|(p, t)| Some((p, t?))).collect();
            (b, input)
        }
        FeatureKind::Fillet { fit, turn, .. } => {
            let (b, u, gone) = fillet_fitted(&mb.body, edges, r, *fit, turn.kernel())?;
            if let Some(u) = &u {
                append_warning(warning, reduced_note(u, r));
            }
            if !gone.is_empty() {
                append_warning(warning, removed_note(&gone));
            }
            used = u;
            (b, input)
        }
        FeatureKind::Chamfer { distance2, angle, flip, .. } => (chamfer_with(vals, &mb.body, edges, r, distance2, angle, *flip)?, input),
        _ => return Err(DocError::Invalid("blend".into())),
    };
    Ok((nb, from, used, tapered))
}

/// `f` as the naming of its faces reads it: a revolve's explicit axis origin written as the values
/// it has now (the naming has no parameter values to read an expression with).
fn naming_feature<'a>(vals: &BTreeMap<String, Value>, f: &'a Feature) -> std::borrow::Cow<'a, Feature> {
    // A cylinder's faces are named by its axis (top, bottom, side), so an axis of expressions
    // goes in as the numbers it is now (#18822).
    if let FeatureKind::Cylinder { axis, .. } = &f.kind
        && crate::literal_dir(axis).is_none()
        && let Ok(d) = direction(vals, axis)
    {
        let mut g = f.clone();
        if let FeatureKind::Cylinder { axis, .. } = &mut g.kind {
            *axis = crate::dir_expr(d);
        }
        return std::borrow::Cow::Owned(g);
    }
    if let FeatureKind::Revolve { axis: AxisRef::Line { origin, .. }, .. } = &f.kind
        && let Ok(o) = point(vals, origin)
    {
        let mut g = f.clone();
        if let FeatureKind::Revolve { axis: AxisRef::Line { origin, .. }, .. } = &mut g.kind {
            *origin = [format!("{} mm", o.x).into(), format!("{} mm", o.y).into(), format!("{} mm", o.z).into()];
        }
        return std::borrow::Cow::Owned(g);
    }
    std::borrow::Cow::Borrowed(f)
}

/// The revolve axis in the sketch's plane coordinates (a point and a direction). `st` is the model
/// an axis on an edge or face of it is found in; `construct` gives a construction axis, by name, in
/// the sketch's 3D frame; `vals` are the parameter values an explicit axis's origin is read with;
/// `doc` with `component` resolves a component origin datum axis as that component sees it
/// ([`crate::construct::model_axis_in`]; `None` for `doc`, as naming resolves without a document);
/// `sketch_line` gives a line of another sketch, by owning sketch feature and curve id, in the
/// same frame as `ss` (#6548).
pub(crate) fn revolve_axis(
    doc: Option<&Document>,
    component: u64,
    vals: &BTreeMap<String, Value>,
    ss: &SolvedSketch,
    axis: &AxisRef,
    st: &ModelState,
    construct: &dyn Fn(&str) -> Result<(Vec3, Vec3)>,
    sketch_line: &dyn Fn(u64, &str) -> Result<(Vec3, Vec3)>,
) -> Result<(Vec2, Vec2)> {
    match axis {
        AxisRef::SketchLine { curve, sketch: None } => {
            let ci = ss.sketch.curve_index(curve).ok_or_else(|| DocError::Unknown(format!("sketch line `{curve}`")))?;
            match ss.sketch.curves.get(ci).map(|c| &c.kind) {
                Some(CurveKind::Line { a, b }) => {
                    let (pa, pb) = (ss.sketch.point(*a).unwrap_or_default(), ss.sketch.point(*b).unwrap_or_default());
                    Ok((pa, pb - pa))
                }
                _ => Err(DocError::Invalid(format!("`{curve}` is not a line"))),
            }
        }
        AxisRef::SketchLine { curve, sketch: Some(owner) } => {
            let (o, d) = sketch_line(*owner, curve)?;
            line_in_sketch_plane(ss, o, d)
        }
        AxisRef::SketchAxis { axis } => match axis.to_ascii_lowercase().as_str() {
            "x" => Ok((Vec2::ZERO, Vec2::X)),
            "y" => Ok((Vec2::ZERO, Vec2::Y)),
            other => Err(DocError::Unknown(format!("sketch axis `{other}`"))),
        },
        AxisRef::World { axis } => {
            let d = match axis.to_ascii_uppercase().as_str() {
                "X" => Vec3::X,
                "Y" => Vec3::Y,
                "Z" => Vec3::Z,
                other => return Err(DocError::Unknown(format!("axis `{other}`"))),
            };
            let pl = &ss.plane;
            if d.dot(pl.normal()).abs() > 1e-9 || pl.height(Vec3::ZERO).abs() > 1e-9 {
                return Err(DocError::Invalid(format!("the {axis} axis does not lie in the sketch plane")));
            }
            Ok((pl.to_local(Vec3::ZERO), Vec2::new(d.dot(pl.x), d.dot(pl.y))))
        }
        AxisRef::Line { origin, dir } => line_in_sketch_plane(ss, point(vals, origin)?, dir.0),
        AxisRef::Model { geo } => {
            let (o, d) = crate::construct::model_axis_in(doc, st, geo, component)?;
            line_in_sketch_plane(ss, o, d)
        }
        AxisRef::Construction { name } => {
            let (o, d) = construct(name)?;
            line_in_sketch_plane(ss, o, d)
        }
    }
}

/// The line `curve` of a solved sketch, as a world-space point and direction in the sketch's
/// plane frame. Used to carry a revolve axis picked in another sketch (#6548).
pub(crate) fn sketch_line_world(ss: &SolvedSketch, curve: &str) -> Result<(Vec3, Vec3)> {
    let ci = ss.sketch.curve_index(curve).ok_or_else(|| DocError::Unknown(format!("sketch line `{curve}`")))?;
    match ss.sketch.curves.get(ci).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => {
            let (pa, pb) = (ss.sketch.point(*a).unwrap_or_default(), ss.sketch.point(*b).unwrap_or_default());
            Ok((ss.plane.to_world(pa), ss.plane.dir_to_world(pb - pa)))
        }
        _ => Err(DocError::Invalid(format!("`{curve}` is not a line"))),
    }
}

/// The model a revolve axis is found in: what construction geometry of component `comp` is built
/// on when the axis is an edge or face of it (only then is it worth placing); a model that cannot
/// be placed is the error.
pub(super) fn axis_state<'a>(doc: &Document, st: &'a ModelState, comp: u64, axis: &AxisRef) -> Result<std::borrow::Cow<'a, ModelState>> {
    match axis {
        AxisRef::Model { .. } => construction_state(doc, st, comp),
        _ => Ok(std::borrow::Cow::Borrowed(st)),
    }
}

/// A world line through `origin` along `dir` in the sketch's plane coordinates, or an error when it
/// does not lie in the sketch plane.
fn line_in_sketch_plane(ss: &SolvedSketch, origin: Vec3, dir: Vec3) -> Result<(Vec2, Vec2)> {
    let pl = &ss.plane;
    let d = dir.normalized().ok_or_else(|| DocError::Invalid("revolve axis has no direction".into()))?;
    if d.dot(pl.normal()).abs() > 1e-9 || pl.height(origin).abs() > 1e-6 {
        return Err(DocError::Invalid("the revolve axis does not lie in the sketch plane".into()));
    }
    Ok((pl.to_local(origin), Vec2::new(d.dot(pl.x), d.dot(pl.y))))
}

/// A construction point `name` of the timeline so far, as evaluated, in the frame `frame` places
/// in the world: the frame a sketch is evaluated in, its component's placement or the placement
/// of the face it is attached to (#12075). The point is kept in its own component's frame.
pub(crate) fn construction_point_in(doc: &Document, st: &ModelState, name: &str, author: u64, frame: &Mat) -> Result<Vec3> {
    let c = st
        .construct
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| DocError::Unknown(format!("construction point `{name}` (it must come earlier in the timeline)")))?;
    let crate::construct::ConstructGeom::Point(p) = &c.geom else {
        return Err(DocError::Invalid(format!("`{name}` is not a construction point")));
    };
    if doc.occurrences.is_empty() {
        return Ok(*p);
    }
    let owner = doc.feature(c.feature).map_or(0, |f| f.component);
    // The same definition is authored in this occurrence, even when it is not the first.
    // Its point is already in the author coordinates; using the first occurrence here
    // would introduce the difference between the two placements.
    if owner == author {
        return Ok(*p);
    }
    let inverse = crate::mat_inverse(frame).ok_or_else(|| DocError::Invalid("the sketch's frame cannot be inverted".into()))?;
    let relative = crate::mat_mul(&inverse, &doc.component_transform(owner));
    Ok(crate::apply_point(&relative, *p))
}

/// A construction axis `name` of the timeline so far, as evaluated, in the frame of component
/// `comp`: a point on it and its direction. Without a document (`doc` None) it is taken in its
/// own component's frame.
pub(crate) fn construction_axis_in(doc: Option<&Document>, st: &ModelState, name: &str, comp: u64) -> Result<(Vec3, Vec3)> {
    let c = st
        .construct
        .iter()
        .find(|c| c.name == name)
        .ok_or_else(|| DocError::Unknown(format!("construction axis `{name}` (it must come earlier in the timeline)")))?;
    let crate::construct::ConstructGeom::Axis { origin, dir } = &c.geom else {
        return Err(DocError::Invalid(format!("`{name}` is not a construction axis")));
    };
    let Some(doc) = doc else { return Ok((*origin, *dir)) };
    let owner = doc.feature(c.feature).map_or(0, |f| f.component);
    if owner == comp || doc.occurrences.is_empty() {
        return Ok((*origin, *dir));
    }
    let inverse =
        crate::mat_inverse(&doc.component_transform(comp)).ok_or_else(|| DocError::Invalid("the feature's component frame is invalid".into()))?;
    let relative = crate::mat_mul(&inverse, &doc.component_transform(owner));
    Ok((crate::apply_point(&relative, *origin), crate::apply_vector(&relative, *dir)))
}

/// Whether a drill from `p` along `d` reaches material: it starts into a body (`touches`), or,
/// when `ahead` is set, a body lies further along `d`. A spot exactly on a surface counts only
/// when the drill enters there, not when it leaves. Only the bodies in `scope` count.
fn hole_reaches(st: &ModelState, p: Vec3, d: Vec3, ahead: bool, scope: &[String]) -> bool {
    let size = st.bodies.iter().map(|b| b.body.size()).fold(1.0, f64::max);
    let eps = size * 1e-6;
    let q = p + d * eps;
    st.bodies.iter().filter(|b| in_hole_scope(scope, b)).any(|b| {
        let m = b.mesh();
        m.contains(q) || (ahead && m.raycast(q, d).is_some())
    })
}

/// The side hole centres on a sketch drill to: into the material, whichever way the sketch plane
/// faces. Material that touches a spot (the drill starts in it) wins over material further away,
/// so a sketch on a plate's face drills that plate rather than one across an air gap: `-n` (away
/// from the plane's normal) if any spot touches material that way, else `+n` if any spot touches
/// material that way. With no touching on either side it goes by the nearest body ahead: `-n` if
/// any spot has a body ahead that way, else `+n` if any has, else `-n`.
/// The way holes at these points of a sketch drill (see [`sketch_hole_dir`]), flipped or not.
/// None when the sketch or a point is not there.
pub fn sketch_hole_direction(st: &ModelState, sketch: u64, ids: &[String], flip: bool) -> Option<Vec3> {
    let ss = st.sketch(sketch)?;
    let spots: Vec<Vec3> =
        ids.iter().map(|id| ss.sketch.points.iter().find(|p| &p.id == id).map(|p| ss.plane.to_world(p.pos))).collect::<Option<_>>()?;
    let dir = sketch_hole_dir(st, spots.iter().copied(), ss.plane.normal(), &[]);
    Some(if flip { -dir } else { dir })
}

fn sketch_hole_dir(st: &ModelState, spots: impl Iterator<Item = Vec3> + Clone, n: Vec3, scope: &[String]) -> Vec3 {
    let down = -n;
    let any = |d: Vec3, ahead: bool| spots.clone().any(|p| hole_reaches(st, p, d, ahead, scope));
    if any(down, false) {
        down
    } else if any(n, false) {
        n
    } else if any(down, true) || !any(n, true) {
        down
    } else {
        n
    }
}

/// The bodies a hole at sketch points finds its side and its faces on: the bodies it cuts when it
/// names them (a hole from a sketch on a body's face cuts that body, #2882), so a body further
/// along can't take the holes meant for it. Empty (every body) otherwise.
fn hole_scope(f: &Feature) -> &[String] {
    match &f.kind {
        FeatureKind::Hole { points: Some(sp), .. } if !sp.single && !f.participants.iter().any(|b| b == "*") => &f.participants,
        _ => &[],
    }
}

/// The bodies whose faces decide which way a hole placed at a position drills, where they pass
/// through it: the bodies it cuts when it names them, so a body touching the face there can't
/// turn it round (#12066). Empty (every body) otherwise, and for `["*"]`.
fn hole_drill_scope(f: &Feature) -> &[String] {
    if f.participants.iter().any(|b| b == "*") { &[] } else { &f.participants }
}

/// Where hole `f`, placed at one spot `p` (a position, or a Single hole's sketch point), starts
/// ([`hole_on_surface`]). A spot on a face of any body starts there. Off every face, it moves along
/// the axis only onto the bodies the hole cuts ([`hole_targets`]): a spot over an opening in its
/// body never slides through it onto a body the hole leaves alone, such as one of another
/// component under the opening (#17358). A body the hole cuts further along the axis still takes
/// it. When the hole cuts every body, or none, every body counts.
pub fn hole_start(st: &ModelState, f: &Feature, p: Vec3, dir: Vec3) -> Vec3 {
    let scope = hole_drill_scope(f);
    // A spot already on a named solid's face stays there. Prove that before asking a
    // nearby reference mesh to classify the spot or intersect the drill axis.
    if !scope.is_empty() && st.bodies.iter().any(|b| b.body.is_mesh()) && on_a_brep_face(st, p, scope) {
        return p;
    }
    let anywhere = hole_on_surface(st, p, dir, &[]);
    // `hole_on_surface` hands `p` itself back when `p` is on a body's boundary (or meets no body
    // at all); the B-rep also says whether it is on a face within the chord tolerance.
    if anywhere == p || on_a_brep_face(st, p, &[]) {
        return p;
    }
    let targets = hole_targets(st, f);
    if targets.len() == st.bodies.len() {
        return anywhere;
    }
    let scope: Vec<String> = targets.iter().map(|b| b.name.clone()).collect();
    hole_on_surface(st, p, dir, &scope)
}

/// Whether `p` lies on a face of a B-rep body, within the body's chord tolerance
/// ([`ModelBody::display_tol`], so the face's mesh is within reach): solidcore decides it on the
/// face's surface and trim ([`solidcraft_kernel::Body::point_lies_on_face`]); the mesh only
/// names the faces to ask.
fn on_a_brep_face(st: &ModelState, p: Vec3, scope: &[String]) -> bool {
    let reach = st.bodies.iter().map(ModelBody::display_tol).fold(0.0, f64::max);
    faces_near(st, p, reach, scope)
        .into_iter()
        .any(|(_, i, face, _)| st.bodies.get(i).is_some_and(|b| matches!(b.body.point_lies_on_face(face as usize, p, b.display_tol()), Ok(Some(_)))))
}

fn in_hole_scope(scope: &[String], b: &ModelBody) -> bool {
    scope.is_empty() || scope.contains(&b.name)
}

/// The sketch points of a hole drilled at several points that its sketch no longer has (#8087):
/// the hole is not drilled there, and drilled at the points that are left.
fn hole_points_gone<'a>(st: &ModelState, f: &'a Feature) -> Vec<&'a str> {
    let FeatureKind::Hole { points: Some(sp), .. } = &f.kind else { return Vec::new() };
    let Some(ss) = st.sketch(sp.sketch.0).filter(|_| !sp.single) else { return Vec::new() };
    sp.ids.iter().filter(|id| !ss.sketch.points.iter().any(|p| &p.id == *id)).map(String::as_str).collect()
}

/// Ids in backticks, comma-separated.
fn backticked<S: AsRef<str>>(ids: &[S]) -> String {
    ids.iter().map(|id| format!("`{}`", id.as_ref())).collect::<Vec<_>>().join(", ")
}

struct HoleSpot<'a> {
    at: Vec3,
    point: Option<&'a str>,
}

fn sketch_hole_spots<'a>(points: &'a crate::SketchPoints, sketch: &crate::SolvedSketch, limit: usize) -> Vec<HoleSpot<'a>> {
    let mut spots: Vec<_> = points
        .ids
        .iter()
        .filter_map(|id| {
            let p = sketch.sketch.points.iter().find(|p| &p.id == id)?;
            Some(HoleSpot { at: sketch.plane.to_world(p.pos), point: Some(id) })
        })
        .take(limit)
        .collect();
    if points.legacy_face_roles && spots.len() == points.ids.len() {
        for spot in &mut spots {
            spot.point = None;
        }
    }
    spots
}

/// Point ids can come from a script or a saved sketch. Keep ordinary ids readable and encode
/// ids that are long or could be read as part of a face name's grammar.
fn hole_point_role(id: &str) -> String {
    if id.len() <= 64 && id.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.')) {
        format!("point:{id}")
    } else {
        use sha2::{Digest, Sha256};
        format!("point-digest:{:x}", Sha256::digest(id.as_bytes()))
    }
}

/// Where a hole feature drills (each on the surface) and in which direction: at its sketch
/// points perpendicular to the sketch (into the material), else at its position. A point gone
/// from the sketch drops only its own hole ([`hole_points_gone`]); with none left it fails.
fn hole_spots<'a>(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, f: &'a Feature) -> Result<(Vec<HoleSpot<'a>>, Vec3)> {
    let FeatureKind::Hole { position, at, direction, points, .. } = &f.kind else { return Err(DocError::Invalid("not a hole".into())) };
    let (mut spots, dir) = match points {
        Some(sp) if !sp.single => {
            let ss =
                st.sketch(sp.sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {} (it must come earlier in the timeline)", sp.sketch)))?;
            let v = sketch_hole_spots(sp, ss, 10_001);
            if v.is_empty() && !sp.ids.is_empty() {
                return Err(DocError::Unknown(format!("sketch point(s) {} (none of the hole's points is left in its sketch)", backticked(&sp.ids))));
            }
            let dir = sketch_hole_dir(st, v.iter().map(|spot| spot.at), ss.plane.normal(), hole_scope(f));
            let dir = if sp.flip { -dir } else { dir };
            (v, dir)
        }
        _ => {
            let dir = direction.normalized().ok_or_else(|| DocError::Invalid("hole direction".into()))?;
            let at = hole_start(st, f, single_hole_position(vals, st, position, at, points)?, dir);
            let at = with_references(vals, st, f, at, dir, true)?;
            let dir = hole_dir_into_face(st, at, dir, hole_drill_scope(f));
            return Ok((vec![HoleSpot { at: hole_start(st, f, at, dir), point: None }], dir));
        }
    };
    if spots.is_empty() || spots.len() > 10_000 {
        return Err(DocError::Invalid("a hole needs 1…10000 points".into()));
    }
    let from_inside = hole_from_inside(doc, f);
    for spot in &mut spots {
        spot.at = hole_on_surface_in(st, spot.at, dir, hole_scope(f), from_inside);
    }
    Ok((spots, dir))
}

/// Whether a hole's spots that lie inside a body move back to its surface: not for the points
/// of a sketch on a derived plane (#11538), unless the hole was saved before that.
fn hole_from_inside(doc: &Document, f: &Feature) -> bool {
    match &f.kind {
        FeatureKind::Hole { old_surface_start: true, .. } => true,
        FeatureKind::Hole { points: Some(sp), .. } if !sp.single => sketch_plane_follows_face(doc, sp.sketch.0),
        _ => true,
    }
}

/// Whether the sketch lies on a picked face, a standard plane (or a plane given outright), so a hole at its points
/// follows that face back to the surface; a sketch on an offset, construction or other derived
/// plane is where the user put it.
fn sketch_plane_follows_face(doc: &Document, sketch: u64) -> bool {
    fn picked(p: &PlaneRef) -> bool {
        match p {
            PlaneRef::Origin { .. } | PlaneRef::Face { .. } | PlaneRef::Custom { .. } | PlaneRef::Placed { .. } => true,
            PlaneRef::Flipped { base, .. } => picked(base),
            _ => false,
        }
    }
    match doc.feature(sketch).map(|f| &f.kind) {
        Some(FeatureKind::Sketch { plane, .. }) => picked(plane),
        _ => true,
    }
}

/// A hole's depth: down to the To Object (where the hole's axis meets the face there, else the
/// point's depth along the axis), else as given.
fn hole_depth_to(st: &ModelState, at: &Vec3, dir: Vec3, to: &Option<Vec3>, depth: &Option<crate::Expr>) -> Result<Option<crate::Expr>> {
    let Some(t) = to else { return Ok(depth.clone()) };
    let mut d = (*t - *at).dot(dir);
    // The face at the To point, met along the axis.
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh()) {
        let Some(fi) = crate::appearance::face_index_at(b, *t) else { continue };
        let m = b.mesh();
        if let Some((h, tri)) = m.raycast(*at + dir * 1e-6, dir)
            && m.tri_face.get(tri).is_some_and(|x| *x as usize == fi)
        {
            d = h + 1e-6;
        }
        break;
    }
    if !(d > 1e-6) {
        return Err(DocError::Invalid("the hole's To object is not ahead of the hole".into()));
    }
    Ok(Some(format!("{d} mm").into()))
}

/// A hole's spots as given (not yet moved onto the surface) and its direction.
fn hole_spots_raw<'a>(vals: &BTreeMap<String, Value>, st: &ModelState, f: &'a Feature) -> Result<(Vec<HoleSpot<'a>>, Vec3)> {
    let FeatureKind::Hole { position, at, direction, points, .. } = &f.kind else { return Err(DocError::Invalid("not a hole".into())) };
    match points {
        Some(sp) if !sp.single => {
            let ss = st.sketch(sp.sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {}", sp.sketch)))?;
            let v = sketch_hole_spots(sp, ss, 10_000);
            let dir = sketch_hole_dir(st, v.iter().map(|spot| spot.at), ss.plane.normal(), hole_scope(f));
            let dir = if sp.flip { -dir } else { dir };
            Ok((v, dir))
        }
        _ => {
            let dir = direction.normalized().ok_or_else(|| DocError::Invalid("hole direction".into()))?;
            let p = single_hole_position(vals, st, position, at, points)?;
            let p = if hole_references(f).is_empty() { p } else { with_references(vals, st, f, hole_start(st, f, p, dir), dir, false)? };
            Ok((vec![HoleSpot { at: p, point: None }], hole_dir_into_face(st, hole_start(st, f, p, dir), dir, hole_drill_scope(f))))
        }
    }
}

/// Where a hole placed by position is drilled: at its sketch point where the sketch has it now,
/// for a Single hole snapped to one (#2110), else as [`hole_position`].
fn single_hole_position(
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    position: &crate::FacePick,
    at: &Option<crate::PointExpr>,
    points: &Option<crate::SketchPoints>,
) -> Result<Vec3> {
    let Some(sp) = points else { return hole_position(vals, position, at) };
    let ss = st.sketch(sp.sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {} (it must come earlier in the timeline)", sp.sketch)))?;
    let id = sp.ids.first().ok_or_else(|| DocError::Invalid("a Single hole needs its sketch point".into()))?;
    let p = ss.sketch.points.iter().find(|p| &p.id == id).ok_or_else(|| DocError::Unknown(format!("sketch point `{id}`")))?;
    Ok(ss.plane.to_world(p.pos))
}

/// A hole's (#37) or a cylinder's (#6380) reference edges; none for any other feature.
fn hole_references(f: &Feature) -> &[crate::HoleReference] {
    match &f.kind {
        FeatureKind::Hole { references, .. } | FeatureKind::Cylinder { references, .. } => references,
        _ => &[],
    }
}

/// Where a cylinder stands: its base, or with reference edges (#6380) at their distances in the
/// plane of the flat face at its base, square to its axis.
/// A box's lowest corner from its stored point and sides: the point itself, or the box's centre
/// when it was placed by its centre (#14487).
fn box_low_corner(at: Vec3, size: Vec3, centred: bool) -> Vec3 {
    if centred { at - size * 0.5 } else { at }
}

fn cylinder_base(vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature, base: &crate::PointExpr, axis: Vec3) -> Result<Vec3> {
    let at = point(vals, base)?;
    with_references(vals, st, f, at, -axis, true)
}

/// Where a Single hole (or a cylinder) with reference edges (#37) is placed: at its distances
/// from the edges' lines, in the plane of the face at `at` (on the surface), the edges found by
/// their names, else their points. A feature without reference edges stays at `at`. With
/// `on_face` the centre must lie on that face; a pattern or mirror replaying a hole that is
/// already drilled finds its centre in the opening and doesn't ask (#8055).
fn with_references(vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature, at: Vec3, dir: Vec3, on_face: bool) -> Result<Vec3> {
    let references = hole_references(f);
    if references.is_empty() {
        return Ok(at);
    }
    let mut distances = Vec::with_capacity(references.len());
    for r in references {
        distances.push(Document::eval_in(vals, &r.distance, Kind::Length)?);
    }
    let edges: Vec<(Vec3, Option<&str>)> = references.iter().enumerate().map(|(i, r)| (r.edge.0, f.edge_names.get(i).map(String::as_str))).collect();
    let frame = crate::hole_reference::reference_frame(st, at, dir, &edges, false)?;
    let centre = frame.place(at, &distances)?;
    if on_face && !crate::hole_reference::on_reference_face(st, &frame, centre) {
        return Err(DocError::Invalid("the reference distances put the hole's centre off its face".into()));
    }
    Ok(centre)
}

/// Where a hole placed by position is drilled: its `at` expressions evaluated, so it follows the
/// parameters (#1678), else the point it was given.
fn hole_position(vals: &BTreeMap<String, Value>, position: &crate::FacePick, at: &Option<crate::PointExpr>) -> Result<Vec3> {
    let Some([x, y, z]) = at else { return Ok(position.0) };
    let p =
        Vec3::new(Document::eval_in(vals, x, Kind::Length)?, Document::eval_in(vals, y, Kind::Length)?, Document::eval_in(vals, z, Kind::Length)?);
    if ![p.x, p.y, p.z].iter().all(|c| c.is_finite() && c.abs() < 1e9) {
        return Err(DocError::Invalid("the hole's position is out of range".into()));
    }
    Ok(p)
}

// Pick an actual cylindrical wall, not a depth derived from its radius: a large pipe bore can
// be shallow or pass through a thin plate. Sampling its finite face also skips a wider entry
// pocket and keeps metadata attached to the narrow cylindrical bore.
//
// A hole drilled across a pocket or a slot leaves its wall in pieces along the axis (#13505):
// every piece the hole reaches (`reach`, its depth from `entry`) is returned, nearest the entry
// first, each as (its start and end from the entry along `inward`, a point on it).
fn parallel_tapped_wall(st: &ModelState, entry: Vec3, inward: Vec3, radius: f64, reach: f64) -> Vec<(f64, f64, Vec3)> {
    let tol = (radius * 1e-6).max(1e-6);
    let mut walls: Vec<(f64, f64, Vec3)> = Vec::new();
    for body in &st.bodies {
        // A wall found lies on the line through `entry` along `inward`, give or take its radius
        // (and the slant its axis may have off `inward` over the body's distance), and its
        // faces lie in the body's exact box: a body whose box that line misses by more holds
        // none, and its faces are not read one by one (#10227).
        let far = more::exact_box(&body.body).map_or(f64::INFINITY, |b| (b.min - entry).len().max((b.max - entry).len()) + b.diagonal());
        if far.is_finite() && more::line_beyond_reach(body, entry, inward, radius * 1.01 + tol + 1e-3 + 2e-4 * far) {
            continue;
        }
        #[cfg(test)]
        cost_tests::count(|c| c.tapped_wall += 1);
        let mesh = body.mesh();
        for face in 0..body.body.face_count() {
            let Some(c) = kernel::cylinder_face(&body.body, &mesh, face) else { continue };
            let offset = entry - c.axis_point;
            if !c.internal
                || (c.radius - radius).abs() > tol
                || c.axis.dot(inward).abs() < 1.0 - 1e-8
                || (offset - c.axis * offset.dot(c.axis)).len() > tol
            {
                continue;
            }
            let a = (c.axis_point + c.axis * c.start - entry).dot(inward);
            let b = (c.axis_point + c.axis * c.end - entry).dot(inward);
            let lo = a.min(b).max(0.0);
            let hi = a.max(b);
            // One piece of wall made of several faces (a cylinder split at its seams) is one.
            let mid = (lo + hi) * 0.5;
            if hi > lo + 1e-9 && lo < reach - 1e-6 && !walls.iter().any(|(l, h, _)| (*l..=*h).contains(&mid)) {
                walls.push((lo, hi, entry + inward * mid + inward.any_perp() * radius));
            }
        }
    }
    walls.sort_by(|a, b| a.0.total_cmp(&b.0));
    walls
}

/// One of a hole feature's tools and the information the sheet metal builder needs to rebuild it.
struct HoleTool {
    traced: Traced,
    prism: Option<more::PrimitivePrism>,
    point_role: Option<String>,
}

/// The cutting tools of a hole feature at every spot, each with its exact geometry when it is a
/// straight cylinder (see [`hole_tools_and_prisms`]).
fn hole_feature_tools(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature) -> Result<Vec<HoleTool>> {
    let FeatureKind::Hole { diameter, depth, hole, to, .. } = &f.kind else { return Err(DocError::Invalid("not a hole".into())) };
    // A hole at sketch points is drilled in its sketch's frame: the bodies it cuts are met where
    // they are shown, so a body of another, moved component is drilled only where the hole
    // reaches it there (#17623).
    let shown = targets_as_shown(st, f, &[])?;
    let st: &ModelState = &shown;
    let (spots, dir) = hole_spots(doc, vals, st, f)?;
    if spots.is_empty() || spots.len() > 10_000 {
        return Err(DocError::Invalid("a hole needs 1…10000 points".into()));
    }
    let kept_inside = !hole_from_inside(doc, f);
    let mesh_scope = hole_mesh_scope(st, hole_drill_scope(f));
    let mut tools = Vec::new();
    for spot in spots {
        let depth = hole_depth_to(st, &spot.at, dir, &to.map(Vec3::from), depth)?;
        let prefix = spot.point.map(hole_point_role);
        tools.extend(
            hole_tools_and_prisms(vals, st, &spot.at, dir, diameter, &depth, hole, kept_inside, mesh_scope)?
                .into_iter()
                .map(|(tool, prism)| HoleTool { traced: tool.with_part_prefix(prefix.as_deref()), prism, point_role: prefix.clone() }),
        );
    }
    Ok(tools)
}

/// A pattern copy's hole tools ([`hole_tools_reaching`]) at `at`, kept for the next copy drilled
/// the same way: along `dir`, `depth` deep, with a `reach` that [`HoleReach::moves_to`] its own.
struct HoleBuilt {
    at: Vec3,
    dir: Vec3,
    depth: Option<crate::Expr>,
    reach: HoleReach,
    tools: Vec<Traced>,
}

/// The scale a hole's tools are sized by: the largest body the hole's axis meets (#2904), so a
/// large body elsewhere in the model changes neither the tool's reach above the face nor its
/// depth. A hole whose axis meets no body is sized by the
/// largest body of the model, as before.
/// Whether `p` may lie in or on `b`: within its exact box grown by the display tolerance, as
/// [`hole_axis_size`] reads it. A point beyond is outside the body.
fn may_hold(b: &ModelBody, p: Vec3) -> bool {
    let Ok(bb) = b.body.exact_bounds() else { return true };
    let pad = 2.0 * b.display_tol();
    !bb.is_empty()
        && (bb.min.x - pad..=bb.max.x + pad).contains(&p.x)
        && (bb.min.y - pad..=bb.max.y + pad).contains(&p.y)
        && (bb.min.z - pad..=bb.max.z + pad).contains(&p.z)
}

/// Use explicit cut targets to decide which meshes take part in material checks. A
/// Combine can consume a target and the cut can follow it to its new body, so retain the
/// full set when any name is missing. An empty scope also covers all-body and implicit cuts.
fn hole_mesh_scope<'a>(st: &ModelState, targets: &'a [String]) -> &'a [String] {
    if targets.iter().all(|name| st.body(name).is_some()) { targets } else { &[] }
}

/// The exact class of `p` in bodies whose bounds [`may_hold`] it. Other solids can back
/// the cut at its exit. Mesh bodies take part when the hole's scope includes them (#21255).
fn hole_point_classes<'a>(st: &'a ModelState, p: Vec3, mesh_scope: &'a [String]) -> impl Iterator<Item = kernel::Result<kernel::PointClass>> + 'a {
    st.bodies.iter().filter(move |b| (!b.body.is_mesh() || in_hole_scope(mesh_scope, b)) && may_hold(b, p)).map(move |b| {
        #[cfg(test)]
        cost_tests::count(|c| c.hole_points += 1);
        b.body.classify_point(p)
    })
}

fn hole_axis_size(st: &ModelState, position: &Vec3, dir: Vec3) -> f64 {
    let along = |b: &ModelBody| {
        // The axis (an unbounded line) against the body's exact box, grown by the display tolerance.
        let bb = b.body.exact_bounds().unwrap_or_else(|_| b.outline_bounds());
        if bb.is_empty() {
            return false;
        }
        let pad = 2.0 * b.display_tol();
        let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
        for (o, d, min, max) in
            [(position.x, dir.x, bb.min.x, bb.max.x), (position.y, dir.y, bb.min.y, bb.max.y), (position.z, dir.z, bb.min.z, bb.max.z)]
        {
            if d.abs() < 1e-12 {
                if o < min - pad || o > max + pad {
                    return false;
                }
            } else {
                let (a, c) = ((min - pad - o) / d, (max + pad - o) / d);
                lo = lo.max(a.min(c));
                hi = hi.min(a.max(c));
            }
        }
        lo <= hi
    };
    let met = st.bodies.iter().filter(|b| along(b)).map(|b| b.body.size()).fold(0.0, f64::max);
    if met > 0.0 { met } else { st.bodies.iter().map(|b| b.body.size()).fold(1.0, f64::max) }
}

/// How close to a hole's entry face material above it may come and still only shorten the
/// tools' run-on (to half the gap, never under 0.1 mm); closer material is cut out of the tools.
const RUN_ON_CLEAR: f64 = 0.2;

/// How far a hole's tools run on above its entry face, and the material there they must leave
/// (#10695). Each tool starts `top` above the face so that it passes cleanly through it, but the
/// material within `reach` of the axis there is not the hole's to cut: a body floating over the
/// face or standing on it, a wall of the part beside the hole, the wall of a bore whose floor the
/// hole is drilled from. Material at least [`RUN_ON_CLEAR`] above the face shortens the run-on to
/// half its gap; material nearer is returned, to be cut out of the tools, with the height above
/// the face of its lowest point (the exact box's, in the hole's frame). The body the hole
/// enters rises above the face's plane around the entry when the face is curved or slants across
/// the axis: those parts of it, touching the entry point, are drilled as before. Exact: a body
/// that may reach above the face is intersected with the run-on's column; one whose exact box
/// lies wholly below the face (the part being drilled, as a rule) costs nothing more. Best
/// effort: where the kernel cannot answer for a body (or for the column), that body keeps the
/// run-on in its way as before, so a hole that drilled before never fails over a neighbour.
fn hole_run_on(st: &ModelState, plane: &Plane, reach: f64, top: f64) -> (f64, Vec<(Body, f64)>) {
    let disc = Region2 { outer: solidcore_record::Loop2::circle_from(Vec2::ZERO, reach, 0.0), holes: vec![] };
    let Some(column) = kernel::extrude(plane, &[disc], 0.0, top).ok().and_then(|mut c| c.pop()) else { return (top, Vec::new()) };
    let column_box = more::exact_box(&column);
    let (o, n, x) = (plane.origin, plane.normal(), plane.x);
    let y = n.cross(x);
    // Into the hole's frame: x and y across the axis, z the height above the face.
    let frame = [[x.x, y.x, n.x, 0.0], [x.y, y.y, n.y, 0.0], [x.z, y.z, n.z, 0.0], [-x.dot(o), -y.dot(o), -n.dot(o), 1.0]];
    let mut lowest = f64::INFINITY;
    let mut near = Vec::new();
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh()) {
        let slack = 2.0 * b.body.record().tolerance + 1e-6;
        // The body's exact box (grown by its tolerance), when the kernel gives one: clear of the
        // column, or wholly at or below the face, it has nothing in the run-on's way.
        let bb = more::exact_box(&b.body);
        if more::boxes_apart(&bb, &column_box) {
            continue;
        }
        if let Some(bb) = bb {
            let corner = |i: u8| {
                Vec3::new(
                    if i & 1 == 0 { bb.min.x } else { bb.max.x },
                    if i & 2 == 0 { bb.min.y } else { bb.max.y },
                    if i & 4 == 0 { bb.min.z } else { bb.max.z },
                )
            };
            if (0..8).all(|i| (corner(i) - o).dot(n) <= 2.0 * slack) {
                continue;
            }
        }
        // The same in the hole's frame, where the box is tight about its axis: a convex face (a
        // radial hole into a round bar) has all its body below the face's plane. Much cheaper
        // than the intersect below; unknown goes on to it.
        if kernel::transform_matrix(&b.body, frame).and_then(|t| t.exact_bounds()).is_ok_and(|t| t.max.z <= 2.0 * slack) {
            continue;
        }
        // The piece of the body in the column and how high its lowest point is, or nothing.
        let found = || -> Result<Option<(Body, f64)>> {
            let Some(piece) = kernel::boolean(&b.body, &column, BoolOp::Intersect)? else { return Ok(None) };
            // The body being drilled, around the entry point: drilled as before. The entry point
            // lies on the face, or just inside the body under a curved face's rim (a counterbore
            // on a round bar measures from the rim's lowest point): the piece over it is the
            // face's own rise, the hole's to cut.
            if piece.classify_point(o)? != kernel::PointClass::Outside {
                let probe = (b.body.size() * 1e-5).max(10.0 * slack);
                if b.body.classify_point(o - n * probe)? == kernel::PointClass::Inside {
                    return Ok(None);
                }
            }
            let low = kernel::transform_matrix(&piece, frame)?.exact_bounds()?.min.z;
            Ok(Some((piece, low)))
        };
        // A kernel error for this body: it keeps the run-on in its way, as before.
        let Ok(Some((piece, low))) = found() else { continue };
        if low >= RUN_ON_CLEAR {
            lowest = lowest.min(low);
        } else {
            near.push((piece, low));
        }
    }
    (if lowest.is_finite() { top.min(lowest / 2.0) } else { top }, near)
}

/// The cutting tools of one hole at `position`, drilling along `dir` (`kept_inside`: see
/// [`hole_reach`]), each with its exact geometry when it drills a straight cylinder along the axis
/// (a plain hole, a counterbore, a drill point left in the air past a through hole; where material
/// above the face was left out of its run-on, the cylinder it still holds whole below that
/// material), so sheet metal can keep it as a flat cut-out or pocket. Each tool's faces are named
/// by the part of the
/// hole it cuts (`bore`, `cbore`, `csink`: `F7:cbore:base` is a counterbore's floor).
#[allow(clippy::too_many_arguments)]
fn hole_tools_and_prisms(
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    position: &Vec3,
    dir: Vec3,
    diameter: &str,
    depth: &Option<crate::Expr>,
    hole: &HoleKind,
    kept_inside: bool,
    mesh_scope: &[String],
) -> Result<Vec<(Traced, Option<more::PrimitivePrism>)>> {
    let reach = hole_reach(vals, st, position, dir, diameter, depth, hole, kept_inside, mesh_scope)?;
    hole_tools_reaching(vals, st, position, dir, diameter, depth, hole, &reach)
}

/// What a hole's tools at one spot take from the model there, not from the hole's numbers: the
/// reach `size` ([`hole_axis_size`]), how far the tools run on above the face (`top`) and the
/// material nearer than that (`above`, cut out of them, each with the height of its lowest point),
/// whether the hole exits the material at its depth (`exits`) and where its bore ends (`bore_end`,
/// past an exit face).
#[derive(Clone, Debug)]
struct HoleReach {
    size: f64,
    top: f64,
    above: Vec<(Body, f64)>,
    exits: bool,
    bore_end: f64,
}

impl HoleReach {
    /// Whether a hole's tools built with this reach, moved, are those built with `other` at the
    /// spot it is moved to (drilled the same way, a hole kind that looks at nothing else there):
    /// the same reach, and no material above either spot, which would be cut out of the tools
    /// where it lies.
    fn moves_to(&self, other: &HoleReach) -> bool {
        self.above.is_empty()
            && other.above.is_empty()
            && self.size == other.size
            && self.top == other.top
            && self.exits == other.exits
            && self.bore_end == other.bore_end
    }
}

/// The [`HoleReach`] of a hole at `position`, drilling along `dir`; `kept_inside` when the spot
/// may stay inside the part (a sketch on a plane through it, #11538), which then starts there.
#[allow(clippy::too_many_arguments)]
fn hole_reach(
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    position: &Vec3,
    dir: Vec3,
    diameter: &str,
    depth: &Option<crate::Expr>,
    hole: &HoleKind,
    kept_inside: bool,
    mesh_scope: &[String],
) -> Result<HoleReach> {
    let r = val(vals, diameter, Kind::Length)? / 2.0;
    if !(r > 1e-6) {
        return Err(DocError::Invalid("hole diameter must be positive".into()));
    }
    let size = hole_axis_size(st, position, dir);
    let mut top = (size * 0.01).max(0.1);
    // A hole whose spot stays inside the part (`kept_inside`: a sketch on a plane through it,
    // #11538) starts there: the reach above a face, there so the tool's top never lies on that
    // face, would cut the material above the plane. Judged at the spot itself, so a plane just
    // under a face keeps the thin roof above it.
    if kept_inside && hole_point_classes(st, *position, mesh_scope).any(|c| matches!(c, Ok(kernel::PointClass::Inside))) {
        top = size * 1e-4;
    }
    // Tools are cylinders and cones made by (tapered) extrudes along the axis, cut one
    // after another, widest first.
    let plane = Plane::from_normal(*position, -dir).ok_or_else(|| DocError::Invalid("hole axis".into()))?;
    // The widest tool's radius at the face, where it runs on above it.
    let wide = match hole {
        HoleKind::Counterbore { cb_diameter: w, .. } | HoleKind::Countersink { cs_diameter: w, .. } => {
            val(vals, w, Kind::Length).map_or(r, |d| (d / 2.0).max(r))
        }
        HoleKind::Simple | HoleKind::Drilled { .. } => r,
    };
    let wide = if wide.is_finite() { wide } else { r };
    // The entry face may slant across the axis: it then rises above the entry point within the
    // tools' reach, and the tools must start clear of all of it, or a wedge of it is left over the
    // low side (#20045).
    let top = top + entry_face_rise(st, position, &plane, wide);
    let (top, above) = hole_run_on(st, &plane, wide, top);
    let bottom = match depth {
        Some(d) => -val(vals, d, Kind::Length)?,
        None => -(size * 2.0 + 1.0),
    };
    if bottom >= 0.0 {
        return Err(DocError::Invalid("hole depth must be positive".into()));
    }
    // A hole exactly as deep as the material (To Object on the face it exits by, or a depth of
    // the wall's thickness) is a through hole. Exact classification in the bodies there: material
    // just above the bottom, air just past it; a point within a body's tolerance of its boundary
    // is neither, and the hole is then not taken to exit there; nor is it where the kernel
    // cannot tell.
    let at = |h: f64| *position + dir * (-h);
    let eps = size * 1e-6;
    let classes = |h: f64| hole_point_classes(st, at(h), mesh_scope).collect::<std::result::Result<Vec<_>, _>>();
    let exits = depth.is_some()
        && classes(bottom + eps).is_ok_and(|c| c.contains(&kernel::PointClass::Inside))
        && classes(bottom - eps).is_ok_and(|c| c.iter().all(|c| *c == kernel::PointClass::Outside));
    // Its bore runs on past the exit face as it does above the entry, never to the material
    // beyond: a floor ending on that face, which is rarely square to the axis to the last 1e-5 rad,
    // crossed it and the cut left half of it across the exit (#17873).
    let bore_end = match exits.then(|| Plane::from_normal(at(bottom), dir)).flatten() {
        Some(exit) => match hole_run_on(st, &exit, r, (size * 0.01).max(0.1)) {
            (run, near) if near.is_empty() => bottom - run,
            _ => bottom,
        },
        None => bottom,
    };
    Ok(HoleReach { size, top, above, exits, bore_end })
}

/// How far a planar face the hole enters by at `position` rises above `plane` (the hole's frame
/// at the entry point, its normal out of the material) within `reach` of the axis: `reach` times
/// the tangent of the face's tilt from square to the axis. Zero for an entry face square to the
/// axis, a curved one (a round bar's: its rim is where the hole starts), or none. A face within 6°
/// of running along the axis is not one the hole enters by, and adds nothing. Exact: the faces are
/// those the B-rep puts the point on ([`solidcraft_kernel::Body::point_lies_on_face`]; the mesh
/// only names the faces to ask) and their planes are the B-rep's.
fn entry_face_rise(st: &ModelState, position: &Vec3, plane: &Plane, reach: f64) -> f64 {
    let n = plane.normal();
    let near = st.bodies.iter().map(ModelBody::display_tol).fold(0.0, f64::max);
    faces_near(st, *position, near, &[])
        .into_iter()
        .filter_map(|(_, i, face, _)| {
            let b = st.bodies.get(i)?;
            b.body.point_lies_on_face(face as usize, *position, b.display_tol()).ok()??;
            let c = b.body.planar_face(face as usize)?.normal().dot(n);
            (c > 0.1).then(|| reach * (1.0 - c * c).max(0.0).sqrt() / c)
        })
        .filter(|rise| rise.is_finite())
        .fold(0.0, f64::max)
}

/// [`hole_tools_and_prisms`] with the hole's [`HoleReach`] at `position` already found.
#[allow(clippy::too_many_arguments)]
fn hole_tools_reaching(
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    position: &Vec3,
    dir: Vec3,
    diameter: &str,
    depth: &Option<crate::Expr>,
    hole: &HoleKind,
    reach: &HoleReach,
) -> Result<Vec<(Traced, Option<more::PrimitivePrism>)>> {
    #[cfg(test)]
    cost_tests::count(|c| c.hole_tools += 1);
    let HoleReach { size, top, ref above, exits, bore_end } = *reach;
    let r = val(vals, diameter, Kind::Length)? / 2.0;
    let plane = Plane::from_normal(*position, -dir).ok_or_else(|| DocError::Invalid("hole axis".into()))?;
    let bottom = match depth {
        Some(d) => -val(vals, d, Kind::Length)?,
        None => -(size * 2.0 + 1.0),
    };
    // Each tool's circle seam at its own angle, so seams don't line up between tools.
    let seam = std::cell::Cell::new(0.0);
    let disc = |rad: f64| {
        seam.set(seam.get() + 0.613);
        Region2 { outer: solidcore_record::Loop2::circle_from(Vec2::ZERO, rad, seam.get()), holes: vec![] }
    };
    let cyl = |rad: f64, lo: f64, hi: f64, part: &'static str| -> Result<(Traced, Option<more::PrimitivePrism>)> {
        let tool = kernel::extrude(&plane, &[disc(rad)], lo, hi)?.pop().ok_or_else(|| DocError::Invalid("hole tool".into()))?;
        let axis = plane.normal();
        let prism = more::PrimitivePrism::Cylinder { base: *position + axis * lo, axis, radius: rad, height: hi - lo, reach: hi - lo, part };
        Ok((crate::lineage::part_tool(tool, part), Some(prism)))
    };
    // Reuse the existing drill-point tool for the narrow bore, independently of its entry.
    let bore = |tip_angle: Option<&crate::Expr>| -> Result<Vec<(Traced, Option<more::PrimitivePrism>)>> {
        if let Some(tip_angle) = tip_angle {
            let half = val(vals, tip_angle, Kind::Angle)? / 2.0;
            // A hole exactly as deep as the material: the point is in the air past the exit
            // face, which would lie on the drill point's shoulder (a through hole, as drilled).
            if depth.is_some() && half > 1e-3 && half < std::f64::consts::FRAC_PI_2 && !exits {
                // One revolved tool (cylinder with a drill point).
                let tip = r / half.tan();
                let up = -dir;
                let side = up.any_perp();
                let rp = Plane::new(*position, side, up).ok_or_else(|| DocError::Invalid("hole axis".into()))?;
                let pts = [Vec2::new(0.0, top), Vec2::new(r, top), Vec2::new(r, bottom), Vec2::new(0.0, bottom - tip)];
                let region = Region2 { outer: solidcore_record::Loop2::polygon(&pts), holes: vec![] };
                return Ok(kernel::revolve(&rp, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?
                    .into_iter()
                    .map(|tool| (crate::lineage::part_tool(tool, "bore"), None))
                    .collect());
            }
        }
        Ok(vec![cyl(r, bore_end, top, "bore")?])
    };
    let mut tools = Vec::new();
    match hole {
        HoleKind::Simple => tools.extend(bore(None)?),
        HoleKind::Drilled { tip_angle } => tools.extend(bore(Some(tip_angle))?),
        HoleKind::Counterbore { cb_diameter, cb_depth, tip_angle, bridge } => {
            let (rc, dc) = (val(vals, cb_diameter, Kind::Length)? / 2.0, val(vals, cb_depth, Kind::Length)?);
            if !(rc > r && dc > 0.0 && -dc > bottom) {
                return Err(DocError::Invalid("the counterbore must be wider than the hole and shallower than it".into()));
            }
            // Wide and shallow first: the narrow hole then crosses the pocket floor.
            tools.push(cyl(rc, -dc, top, "cbore")?);
            if let Some(layer) = bridge {
                let layer = val(vals, layer, Kind::Length)?;
                tools.extend(bridge_tools(&plane, r, rc, dc, bottom, layer)?);
                // Through all, `bottom` is the tool's reach, not the material's: the second layer
                // must end inside a body (exact classification), short of the exit face.
                if depth.is_none() {
                    let past = *position + dir * (dc + 2.0 * layer + size * 1e-6);
                    let classes = hole_point_classes(st, past, &[]).collect::<std::result::Result<Vec<_>, _>>()?;
                    if !classes.contains(&kernel::PointClass::Inside) {
                        return Err(bridge_too_deep(layer, "make the counterbore shallower or the bridge thinner"));
                    }
                }
            }
            tools.extend(bore(tip_angle.as_ref())?);
        }
        HoleKind::Countersink { cs_diameter, cs_angle, tip_angle } => {
            let (rc, half) = (val(vals, cs_diameter, Kind::Length)? / 2.0, val(vals, cs_angle, Kind::Angle)? / 2.0);
            // The angle and the width are told apart: an angle out of range is not a countersink
            // too narrow (#9625). The cone's length grows as 1 / tan(half), so a near-flat angle
            // is refused at a floor the message names, rather than built kilometres long.
            const MIN_ANGLE_DEG: f64 = 0.115;
            if !(2.0 * half > MIN_ANGLE_DEG.to_radians() && half < std::f64::consts::FRAC_PI_2) {
                return Err(DocError::Invalid(format!(
                    "the countersink angle must be more than {MIN_ANGLE_DEG}° and less than 180°, not {}°",
                    ((2.0 * half).to_degrees() * 1e6).round() / 1e6
                )));
            }
            if !(rc > r) {
                return Err(DocError::Invalid("the countersink must be wider than the hole".into()));
            }
            // One revolved tool: the cone from its rim on the face to just inside the hole
            // radius, and above the face the rim's cylinder, as the bore's own cylinder runs on
            // above it. The cone widening on above the face would cut what stands beside the rim
            // (#5041). The cone is the profile's first segment (`csink:side:0`).
            // The cone stops at the hole's depth when that comes first: a countersink deeper
            // than its hole is cut to the hole (#10697).
            let reach = (rc - r * 0.9) / half.tan();
            let len = reach.min(-bottom);
            let r_end = rc - len * half.tan();
            let up = -dir;
            let rp = Plane::new(*position, up.any_perp(), up).ok_or_else(|| DocError::Invalid("hole axis".into()))?;
            let pts = [Vec2::new(rc, 0.0), Vec2::new(r_end, -len), Vec2::new(0.0, -len), Vec2::new(0.0, top), Vec2::new(rc, top)];
            let region = Region2 { outer: solidcore_record::Loop2::polygon(&pts), holes: vec![] };
            tools.extend(
                kernel::revolve(&rp, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?
                    .into_iter()
                    .map(|tool| (crate::lineage::part_tool(tool, "csink"), None)),
            );
            tools.extend(bore(tip_angle.as_ref())?);
        }
    }
    if above.is_empty() {
        return Ok(tools);
    }
    // The material above the face the run-on would reach, left out of each tool it meets; such
    // a tool is no longer a plain cylinder. It still holds its cylinder whole up to the lowest
    // point of what was left out of it, which lies at or above the face: its exact geometry keeps
    // that much, all it drills in the material it enters, and sheet metal drills only that. The
    // whole cylinder stays its reach (it takes a drill that meets one panel and nothing else of
    // the sheet).
    let mut kept = Vec::with_capacity(tools.len());
    for (mut tool, mut prism) in tools {
        for (piece, low) in above {
            // Best effort: where the kernel cannot answer, the tool stays as it was before.
            if !matches!(kernel::boolean(&tool.body, piece, BoolOp::Intersect), Ok(Some(_))) {
                continue;
            }
            let Ok(Some(cut)) = kernel::boolean(&tool.body, piece, BoolOp::Cut) else { continue };
            tool = Traced::step(cut, &[&tool, &Traced::unnamed(piece.clone())]);
            // The face lies `(position - base) · axis` along the cylinder from its base, and what
            // was left out lies in the run-on's column, at or above it.
            if let Some(more::PrimitivePrism::Cylinder { base, axis, height, .. }) = &mut prism {
                *height = height.min((*position - *base).dot(*axis) + low.max(0.0));
            }
        }
        kept.push((tool, prism));
    }
    Ok(kept)
}

/// How much wider than the hole a bridged counterbore's slot and square are, in mm.
const BRIDGE_CLEARANCE: f64 = 0.02;

/// Why a bridged counterbore's two layers of `layer` were refused: they reach past the hole, and
/// `fix` says what to change.
fn bridge_too_deep(layer: f64, fix: &str) -> DocError {
    DocError::Invalid(format!("the two bridge layers ({:.3} mm) must fit in the hole below the counterbore: {fix}", 2.0 * layer))
}

/// The bridge layers of a counterbore printed opening down: below its floor, a slot across the
/// whole counterbore, along X (one `layer` thick), then a square (one more), both
/// [`BRIDGE_CLEARANCE`] wider than the hole, so each layer spans between supported edges.
/// `plane` is the hole's, its normal out of the material; the bore crosses both layers, clear of
/// their sides.
fn bridge_tools(plane: &Plane, r: f64, rc: f64, dc: f64, bottom: f64, layer: f64) -> Result<Vec<(Traced, Option<more::PrimitivePrism>)>> {
    if !(layer > 0.0 && layer.is_finite()) {
        return Err(DocError::Invalid("the bridge layer must be a positive length, such as 0.2 mm".into()));
    }
    let (t1, t2) = (-dc - layer, -dc - 2.0 * layer);
    if t2 <= bottom {
        return Err(bridge_too_deep(layer, "make the hole deeper or the bridge thinner"));
    }
    // Both a hair wider than the hole, so the bore's rim doesn't touch their sides.
    let q = r + BRIDGE_CLEARANCE / 2.0;
    // The slot's ends need room on the counterbore's wall past that.
    if !(rc >= q + BRIDGE_CLEARANCE / 2.0) {
        return Err(DocError::Invalid(format!("a bridged counterbore must be at least {} mm wider than the hole", 2.0 * BRIDGE_CLEARANCE)));
    }
    // The slot along world X where the axis allows it, else along the hole plane's own x.
    let n = plane.normal();
    let x = (Vec3::X - n * Vec3::X.dot(n)).normalized().filter(|_| Vec3::X.dot(n).abs() < 0.99).unwrap_or(plane.x);
    let bp = Plane::new(plane.origin, x, n.cross(x)).ok_or_else(|| DocError::Invalid("hole axis".into()))?;
    // The slot ends on the counterbore's wall: two lines and two arcs of its circle.
    let a = (rc * rc - q * q).sqrt();
    let phi = q.atan2(a);
    let slot = solidcore_record::Loop2 {
        segs: vec![
            solidcore_record::Seg2::Line { a: Vec2::new(-a, -q), b: Vec2::new(a, -q) },
            solidcore_record::Seg2::Arc { center: Vec2::ZERO, radius: rc, start: -phi, sweep: 2.0 * phi },
            solidcore_record::Seg2::Line { a: Vec2::new(a, q), b: Vec2::new(-a, q) },
            solidcore_record::Seg2::Arc { center: Vec2::ZERO, radius: rc, start: std::f64::consts::PI - phi, sweep: 2.0 * phi },
        ],
    };
    let square = solidcore_record::Loop2::polygon(&[Vec2::new(-q, -q), Vec2::new(q, -q), Vec2::new(q, q), Vec2::new(-q, q)]);
    let mut tools = Vec::new();
    for (outline, lo, hi) in [(slot, t1, -dc), (square, t2, t1)] {
        let region = Region2 { outer: outline, holes: vec![] };
        let tool = kernel::extrude(&bp, &[region], lo, hi)?.pop().ok_or_else(|| DocError::Invalid("hole tool".into()))?;
        tools.push((crate::lineage::part_tool(tool, "bridge"), None));
    }
    Ok(tools)
}

/// The turn a whole turn's tool is turned by about the revolve axis, so that its seam is not in
/// the profile plane: a body revolved in that plane has its own seam there, and SMLib's boolean
/// leaves an edge near the seam when it splits the two (#18039). The solid is the same (a whole
/// turn is the same whatever angle it starts at).
const SEAM_TURN: f64 = 0.5;

/// Whether `body` holds a seam in the plane `plane`: an edge of its B-rep that is a seam of a
/// curved surface and lies in the plane. Two solids of revolution about the same axis, revolved
/// in the same plane, hold their seams in that plane, and SMLib's boolean splits both where the
/// cut crosses them and leaves an edge near the seam, which its check refuses (#18039).
///
/// The seam is read from the exact record, never from a tessellation: the kernel's own seam
/// flags (which the record's faces and edges decide) and the record's own vertices. The seam of
/// a surface of revolution is the profile curve in the revolve's plane, so its ends in the plane
/// mean the seam lies in it.
fn has_seam_in_plane(body: &Body, plane: &Plane) -> bool {
    // The kernel's seam flags are read from the record; the mesh it takes with them is unused.
    let seams = kernel::seam_flags(body, &Mesh::default());
    let record = body.record();
    let tolerance = body.size().max(1e-9) * 1e-6;
    seams.iter().enumerate().any(|(index, seam)| {
        *seam
            && record.edges.get(index).is_some_and(|edge| {
                edge.vertices
                    .iter()
                    .all(|vertex| record.vertices.get(*vertex as usize).is_some_and(|v| plane.height(Vec3::from(v.point)).abs() <= tolerance))
            })
    })
}

/// The tool of a whole turn, with the seam of its surface of revolution turned away from the
/// profile plane: the same solid, with the seam somewhere else, so the two seams are not in one
/// plane (#18039).
fn turned_seam(axis: Vec3, origin: Vec3, body: &Body) -> Result<Body> {
    let m = crate::rigid(Vec3::ZERO, origin, axis, SEAM_TURN);
    kernel::transform_matrix(body, m).map_err(DocError::Kernel)
}

/// Whether any of the bodies the feature cuts holds a seam in the plane `plane`.
fn bodies_hold_a_seam_in_plane(st: &ModelState, f: &Feature, plane: &Plane) -> bool {
    let FeatureKind::Revolve { targets, .. } = &f.kind else { return false };
    st.bodies
        .iter()
        .filter(|b| b.component == f.component)
        .filter(|b| targets.is_empty() || targets.iter().any(|t| t == &b.name))
        .any(|b| has_seam_in_plane(&b.body, plane))
}

/// The angular interval used by both the solid and its interactive extent handles.
#[derive(Clone, Copy, Debug)]
pub struct RevolveExtent {
    pub start: f64,
    pub sweep: f64,
    pub rotation_axis: Vec3,
    pub display_start: f64,
    /// A curved target trims the swept solid and has no single angular boundary.
    pub curved_to: bool,
}

pub fn revolve_extent(
    st: &ModelState,
    plane: &Plane,
    regions: &[Region2],
    (origin, axis): (Vec3, Vec3),
    (angle, angle2): (f64, Option<f64>),
    symmetric: bool,
    to: Option<Vec3>,
) -> Result<RevolveExtent> {
    revolve_extent_with_convention(st, plane, regions, (origin, axis), (angle, angle2), symmetric, to, crate::RevolveConvention::LegacyAxisStart)
}

#[allow(clippy::too_many_arguments)]
pub fn revolve_extent_with_convention(
    st: &ModelState,
    plane: &Plane,
    regions: &[Region2],
    (origin, axis): (Vec3, Vec3),
    (angle, angle2): (f64, Option<f64>),
    symmetric: bool,
    to: Option<Vec3>,
    convention: crate::RevolveConvention,
) -> Result<RevolveExtent> {
    let extent = revolve_extent_for_handle(st, plane, regions, (origin, axis), (angle, angle2), symmetric, to, convention)?;
    // A solid needs an open sweep inside one full turn. Zero and a turn past 360° keep a
    // handle frame (`revolve_extent_for_handle`) and still fail here, with the same message.
    if !(extent.sweep.abs() > 1e-9 && extent.sweep.abs() <= std::f64::consts::TAU + 1e-9) {
        return Err(DocError::Invalid("the revolve angle must be more than 0 and at most 360°".into()));
    }
    Ok(extent)
}

/// Where the angle handle sits. A sweep of zero, or past a full turn, builds no solid and
/// [`revolve_extent_with_convention`] refuses it. The boundary is still a point on the circle,
/// and the grab point stays there so the angle can be dragged back into range.
#[allow(clippy::too_many_arguments)]
pub fn revolve_extent_for_handle(
    st: &ModelState,
    plane: &Plane,
    regions: &[Region2],
    (origin, axis): (Vec3, Vec3),
    (angle, angle2): (f64, Option<f64>),
    symmetric: bool,
    to: Option<Vec3>,
    convention: crate::RevolveConvention,
) -> Result<RevolveExtent> {
    let (start, sweep) = match (to, angle2, symmetric) {
        (Some(point), _, _) => {
            let points: Vec<Vec2> = regions.iter().flat_map(|r| r.outer.polyline(1e-2)).collect();
            let centre = if convention == crate::RevolveConvention::SketchNormal {
                regions.first().map(|r| plane.to_world(r.centroid())).ok_or_else(|| DocError::Invalid("no profile selected".into()))?
            } else {
                plane.to_world(points.iter().fold(Vec2::default(), |sum, point| sum + *point) * (1.0 / points.len().max(1) as f64))
            };
            let projection_axis = if convention == crate::RevolveConvention::SketchNormal {
                axis.normalized().ok_or_else(|| DocError::Invalid("revolve axis has no direction".into()))?
            } else {
                axis
            };
            let flat = |point: Vec3| {
                let v = point - origin;
                v - projection_axis * v.dot(projection_axis)
            };
            let (u, v) = (flat(centre), flat(point));
            let angle_axis = if convention == crate::RevolveConvention::SketchNormal {
                if v.len() < 1e-9 {
                    return Err(DocError::Invalid("the revolve target lies on the axis; pick a point away from the axis".into()));
                }
                kernel::canonical_revolve_axis(
                    plane,
                    regions.first().ok_or_else(|| DocError::Invalid("no profile selected".into()))?,
                    plane.to_local(origin),
                    Vec2::new(projection_axis.dot(plane.x), projection_axis.dot(plane.y)),
                )?
            } else {
                -axis
            };
            let sweep = angle_axis.dot(u.cross(v)).atan2(u.dot(v));
            (0.0, if sweep <= 1e-9 { sweep + std::f64::consts::TAU } else { sweep })
        }
        (None, Some(other), _) => (-other, angle + other),
        (None, None, true) => (-angle / 2.0, angle),
        (None, None, false) => (0.0, angle),
    };
    if !(sweep.is_finite() && start.is_finite()) {
        return Err(DocError::Invalid("the revolve angle must be more than 0 and at most 360°".into()));
    }
    let points: Vec<Vec2> = regions.iter().flat_map(|r| r.outer.polyline(1e-2)).collect();
    let centre = plane.to_world(points.iter().fold(Vec2::default(), |sum, point| sum + *point) * (1.0 / points.len().max(1) as f64));
    let unit_axis = axis.normalized().ok_or_else(|| DocError::Invalid("revolve axis has no direction".into()))?;
    let radial_centre = regions.first().map(|region| plane.to_world(region.centroid())).unwrap_or(centre);
    let radial = radial_centre - origin - unit_axis * (radial_centre - origin).dot(unit_axis);
    let rotation_axis = match (convention, regions.first()) {
        (crate::RevolveConvention::SketchNormal, Some(region)) => {
            kernel::canonical_revolve_axis(plane, region, plane.to_local(origin), Vec2::new(unit_axis.dot(plane.x), unit_axis.dot(plane.y)))?
        }
        _ => {
            if unit_axis.cross(radial).dot(plane.normal()) < 0.0 {
                -unit_axis
            } else {
                unit_axis
            }
        }
    };
    let display_start = match convention {
        crate::RevolveConvention::LegacyAxisStart => start * -unit_axis.dot(rotation_axis),
        crate::RevolveConvention::SketchNormal => start,
    };
    Ok(RevolveExtent { start, sweep, rotation_axis, display_start, curved_to: to.is_some_and(|point| curved_target_any(st, point).is_some()) })
}

/// The exact input frame and scalar interval of a current, untrimmed constructor.
struct ExtrudeConstructor {
    plane: Plane,
    interval: (f64, f64),
}

/// How far from its sketch plane an extrude may reach, in mm. It mirrors the limit the kernel's
/// `finite` check in `crates/kernel/src/build.rs` puts on an extrusion's start and end (a literal
/// there, not exported), so a too-long extrusion is refused here, by name, before the kernel is
/// called: the kernel's own message, `end = 1000000000`, names no field (#9625).
const MAX_EXTRUDE_REACH: f64 = 1e7;

/// Refuse an extrusion whose ends lie past [`MAX_EXTRUDE_REACH`] from the sketch plane.
fn extrude_within_reach(ends: [f64; 2]) -> Result<()> {
    match ends.into_iter().find(|x| x.abs() >= MAX_EXTRUDE_REACH) {
        Some(x) => Err(DocError::Invalid(format!(
            "the extrusion is too long: its end is {x} mm from the sketch plane, past the limit of {MAX_EXTRUDE_REACH} mm"
        ))),
        None => Ok(()),
    }
}

/// Construct planar caps locally before placing the completed tool.
#[derive(Clone, Copy)]
enum CapRoles {
    ByHeight,
    TwoSided,
    Named([&'static str; 2]),
}

fn centered_extrude(
    plane: &Plane,
    region: &Region2,
    start: f64,
    end: f64,
    sketch: Option<&SolvedSketch>,
    thin: Option<&crate::lineage::ThinWallRoles>,
    cap_roles: CapRoles,
    one_sided_direction: Option<crate::Direction>,
) -> Result<Vec<Traced>> {
    if !plane.origin.is_finite()
        || !plane.x.is_finite()
        || !plane.y.is_finite()
        || (plane.x.len2() - 1.0).abs() > 1e-8
        || (plane.y.len2() - 1.0).abs() > 1e-8
        || plane.x.dot(plane.y).abs() > 1e-8
    {
        return Err(DocError::Invalid("extrude frame is not finite and orthonormal".into()));
    }
    // The faces' roles: the sketch curve under each side, the caps by where they lie.
    let (lo, hi) = if start <= end { (start, end) } else { (end, start) };
    // A two-sided extrude has no cap on its sketch plane: its two caps close the two ends of
    // the tool, the low one downwards and the high one upwards, so they are named by the end
    // they close. Their height cannot tell them apart: both sides may lie on the same side of
    // the sketch plane, and two caps named alike are two pieces of one face, which the
    // history cannot tell apart (a sketch on either of them could not keep its attachment).
    let caps = match cap_roles {
        CapRoles::TwoSided => [Some("bottom".to_string()), Some("top".to_string())],
        CapRoles::Named([a, b]) => [Some(a.to_string()), Some(b.to_string())],
        CapRoles::ByHeight => [Some(crate::lineage::extrude_cap_role(lo)), Some(crate::lineage::extrude_cap_role(hi))],
    };
    // An extrude that spans its sketch plane (symmetric) has no cap on it. One made from a
    // one-sided extrude knows the way that one went ([`crate::Extent::one_sided_direction`]):
    // its cap at the end where the old start cap lay, facing the same way, also answers to
    // `start`, so a reference to `start` picked before still finds that cap and never the
    // other. Without that record no cap answers to `start`. It spans the plane when its caps
    // lie on both sides of it, as their roles already say.
    let spans = caps[0].as_deref() == Some("bottom") && caps[1].as_deref() == Some("top") && !matches!(cap_roles, CapRoles::TwoSided);
    let start_name = || Some("start".to_string());
    let cap_synonyms = match one_sided_direction.filter(|_| spans) {
        Some(crate::Direction::Positive) => [start_name(), None],
        Some(crate::Direction::Negative) => [None, start_name()],
        Some(crate::Direction::Symmetric) | None => [None, None],
    };
    let roles = crate::lineage::ProfileRoles { sketch, region: Some(region), caps, cap_synonyms, thin };
    // Fit cap surfaces in their own coordinates. Fitting a distant world-space
    // wire can tilt an otherwise exact planar cap through numeric cancellation.
    // A side drawn in collinear pieces is one face (#1788), and so is a side along one ellipse
    // or spline, whatever pieces the profile holds it in.
    let curves = sketch.map(crate::config::ExactSketchCurves::new);
    let one_curve = |s: &solidcore_record::Seg2, t: &solidcore_record::Seg2| {
        curves.as_ref().is_some_and(|c| c.find_free_form(s).is_some_and(|id| c.find_free_form(t).is_some_and(|other| other == id)))
    };
    let bodies = kernel::extrude_whole_sides(&Plane::XY, std::slice::from_ref(region), start, end, &one_curve)?;
    if *plane == Plane::XY {
        return Ok(bodies.into_iter().map(|body| roles.tool(body, None)).collect());
    }
    let n = plane.normal();
    let m = [
        [plane.x.x, plane.x.y, plane.x.z, 0.0],
        [plane.y.x, plane.y.y, plane.y.z, 0.0],
        [n.x, n.y, n.z, 0.0],
        [plane.origin.x, plane.origin.y, plane.origin.z, 1.0],
    ];
    bodies
        .into_iter()
        .map(|body| {
            let moved = kernel::transform_matrix(&body, m)?;
            Ok(roles.tool(moved, Some(&body)))
        })
        .collect()
}

/// How far a thin extrude's wall reaches either side of its curves: (inside, outside).
fn thin_distances(vals: &BTreeMap<String, Value>, thin: &crate::ThinWall) -> Result<(f64, f64)> {
    let t = val(vals, &thin.thickness, Kind::Length)?;
    let (inside, outside) = match thin.side {
        crate::ThinSide::Center => (t / 2.0, t / 2.0),
        crate::ThinSide::Inside => (t, 0.0),
        crate::ThinSide::Outside => (0.0, t),
        crate::ThinSide::TwoSides => {
            let e = thin.thickness2.as_deref().ok_or_else(|| DocError::Invalid("a two-sided thin wall needs `thickness2`".into()))?;
            (t, val(vals, e, Kind::Length)?)
        }
    };
    if !(inside >= 0.0 && outside >= 0.0 && inside + outside > 0.0) {
        return Err(DocError::Invalid("the wall thickness must be positive".into()));
    }
    Ok((inside, outside))
}

/// The regions a thin extrude's wall covers in its sketch.
pub(crate) fn thin_regions(vals: &BTreeMap<String, Value>, ss: &SolvedSketch, thin: &crate::ThinWall) -> Result<Vec<Region2>> {
    if thin.curves.is_empty() || thin.curves.len() > 1000 {
        return Err(DocError::Invalid("a thin extrude needs 1…1000 sketch curves".into()));
    }
    let (inside, outside) = thin_distances(vals, thin)?;
    let curves = thin
        .curves
        .iter()
        .map(|id| {
            let ci = ss.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("curve `{id}` of {}", ss.name)))?;
            Ok(ss.sketch.exact_segs(ci).unwrap_or_else(|| ss.sketch.segs(ci)))
        })
        .collect::<Result<Vec<_>>>()?;
    solidcraft_sketch::thin_wall_regions(&curves, inside, outside).map_err(|e| DocError::Invalid(format!("thin wall: {e}")))
}

/// How far apart (mm) profiles must stay for their extruded tools to be cut as one.
const APART_GAP: f64 = 1e-3;

/// The most shells one tool of profiles apart holds: a body's record takes no more, so a sketch
/// of 1369 separate squares, joined into one tool, failed with "shell count" (#20398).
const MAX_JOINED_SHELLS: usize = solidcore_record::brep::MAX_SHELLS;

#[cfg(test)]
thread_local! {
    /// [`MAX_JOINED_SHELLS`] for this thread's tests: a test splits a few profiles into groups.
    static JOINED_SHELLS_LIMIT: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_JOINED_SHELLS) };
}

#[cfg(test)]
fn joined_shells_limit() -> usize {
    JOINED_SHELLS_LIMIT.with(std::cell::Cell::get)
}

#[cfg(not(test))]
fn joined_shells_limit() -> usize {
    MAX_JOINED_SHELLS
}

/// Tools apart from each other (extruded from profiles apart in the sketch) as one tool, so the
/// body is cut or joined once rather than once per profile. Tools of more than
/// [`MAX_JOINED_SHELLS`] shells together are joined in even groups, one tool for each, and the
/// body is cut or joined once per group (#20398). Tools the kernel does not join (meshes) are
/// left as they are.
fn join_apart_tools(tools: Vec<Traced>) -> Result<Vec<Traced>> {
    let limit = joined_shells_limit().max(1);
    let shells: Vec<usize> = tools.iter().map(|t| t.body.record().shells.len().max(1)).collect();
    let total: usize = shells.iter().sum();
    // Even groups: as many as the limit asks for, each holding about as many shells.
    let most = total.div_ceil(total.div_ceil(limit).max(1));
    let mut groups: Vec<Vec<Traced>> = Vec::new();
    let mut held = 0;
    for (tool, n) in tools.into_iter().zip(shells) {
        match groups.last_mut() {
            Some(group) if held + n <= most => {
                group.push(tool);
                held += n;
            }
            _ => {
                groups.push(vec![tool]);
                held = n;
            }
        }
    }
    let mut out = Vec::with_capacity(groups.len());
    for group in groups {
        let bodies: Vec<Body> = group.iter().map(|t| t.body.clone()).collect();
        match kernel::join_apart(&bodies)? {
            Some(one) => out.push(Traced::step(one, &group.iter().collect::<Vec<_>>())),
            None => out.extend(group),
        }
    }
    Ok(out)
}

/// The tool bodies a feature adds or removes (extrude, revolve, primitives), in the current state.
/// `to_body` is the body a To Object extent runs to (an extrude), when it found one.
fn feature_tools_with_constructor(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    constructor: &mut Option<ExtrudeConstructor>,
    to_body: &mut Option<String>,
    warning: &mut Option<String>,
) -> Result<Vec<Traced>> {
    *to_body = None;
    let unnamed = |bodies: Vec<Body>| -> Vec<Traced> { bodies.into_iter().map(Traced::unnamed).collect() };
    match &f.kind {
        FeatureKind::Extrude { sketch, profiles, extent, operation, targets, thin } => {
            let ss = constructor_profile_in_frame(
                doc,
                st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?,
                f,
            )?;
            let regions = match thin {
                Some(thin) => thin_regions(vals, &ss, thin)?,
                None => solidcraft_sketch::merge_regions(&select_profiles(&ss, profiles)?),
            };
            // A thin wall's sides are named by the curves they run beside.
            let thin_roles = match thin {
                Some(thin) => {
                    let (inside, outside) = thin_distances(vals, thin)?;
                    Some(crate::lineage::ThinWallRoles { curves: thin.curves.clone(), distances: [inside, outside] })
                }
                None => None,
            };
            // From / To Object: where the face or point lies along the normal, measured over the
            // middle of the profiles. The points are in this feature's component frame, so the
            // model is seen from there: a target in another, moved component where it is shown.
            let n = ss.plane.normal();
            let tst = if extent.to.is_some() || extent.from.is_some() {
                state_in_frame_pinned_to(doc, st, f.component, &extent.picked_placements)?
            } else {
                std::borrow::Cow::Borrowed(st)
            };
            // Through all, and where the tool meets its targets, is measured where they are shown.
            let shown = targets_as_shown(st, f, &crate::plain(targets))?;
            let st: &ModelState = &shown;
            // The face names follow `face_points`: To, then From.
            let name = |k: usize| f.face_names.get(k).map(String::as_str);
            let (to_name, from_name) = if extent.to.is_some() { (name(0), name(1)) } else { (None, name(0)) };
            let to_face = extent.to.and_then(|p| object_face_at_level(&tst, to_name, p.0, &ss.plane));
            // A vertex or an edge point where a plane the extrusion runs along meets another face:
            // its own level is the end (#12968), not the surface of one of the faces there.
            let to_end_face = to_face.filter(|face| !extent.to.is_some_and(|p| on_edge_without_cap(&tst, *face, p.0, n)));
            // The body the extent runs to: a Join to it joins that body too (#6424), unless the
            // design was built before it did (#8151).
            *to_body = to_face.filter(|_| !extent.to_body_apart).and_then(|(bi, _)| tst.bodies.get(bi)).map(|b| b.name.clone());
            let centre = {
                let pts: Vec<Vec2> = regions.iter().flat_map(|r| r.outer.polyline(1e-2)).collect();
                let c = pts.iter().fold(Vec2::default(), |a, p| a + *p) * (1.0 / pts.len().max(1) as f64);
                ss.plane.to_world(c)
            };
            let from_face = extent.from.and_then(|p| object_face_at_level(&tst, from_name, p.0, &ss.plane));
            let from = match extent.from {
                Some(p) => height_at(&tst, from_face, &ss.plane, centre, p.0),
                None => 0.0,
            };
            // From Object on a flat face at a slant starts on the face, so both ends follow its
            // slope, unless the design was built when it started at the face's level. With To
            // Object on a curved face, through all or a taper, or on a face within 5 degrees of
            // running along the extrusion (where the profile's shadow on it is long and thin), it
            // still starts at that level, and the feature's warning says so.
            let slanted_from = from_face.filter(|_| !extent.from_level).and_then(|face| slanted_plane(&tst, face, n));
            // A design kept on the level start with To Object still says why it starts at the
            // level: one saved before From + To on flat faces ran between them, or before format
            // 58, when every From started at the level.
            let kept_slope = from_face.filter(|_| extent.from_level && extent.to.is_some()).and_then(|face| slanted_plane(&tst, face, n));
            let tapered = match &extent.taper {
                Some(tp) => val(vals, tp, Kind::Angle)? != 0.0,
                None => false,
            };
            // The plane To Object ends on, when it is flat: a face's plane, square to the
            // extrusion or at a slant to it (but not within 5 degrees of running along it), or
            // the plane parallel to the sketch through a point or an edge (#18825).
            let to_end_plane = extent.to.and_then(|p| flat_to_end(&tst, to_end_face, &ss.plane, centre, p.0));
            let ends = ExtrudeEnds {
                to: extent.to.map(|_| to_end_plane.is_some()),
                through_all: extent.through_all,
                tapered,
                two_sided: extent.distance2.is_some(),
            };
            let level_because = match (&slanted_from, &kept_slope) {
                (Some(slope), _) => level_start_reason(slope, n, &ends),
                (None, Some(slope)) => {
                    Some(level_start_reason(slope, n, &ends).unwrap_or("the design was saved before the start followed the slanted face"))
                }
                (None, None) => None,
            };
            if let Some(why) = level_because {
                append_warning(
                    warning,
                    format!(
                        "From Object starts on a plane parallel to the sketch, where the slanted face meets the middle of the profiles, not on the face itself, because {why}"
                    ),
                );
            }
            let from_slope = slanted_from.filter(|_| level_because.is_none());
            // From a slanted face To a flat face or a point: the extrude runs from the From
            // face's plane to the To one's, each moved along the extrusion by its offset, and the
            // way it runs is read where they meet the line through the profiles' middle (#18825).
            // The planes and the signed distance between them there.
            let from_to = match (&from_slope, &to_end_plane) {
                (Some(fp), Some(tp)) if extent.to.is_some() => {
                    let start_offset = extent.start_offset.as_deref().map(|e| val(vals, e, Kind::Length)).transpose()?.unwrap_or(0.0);
                    let to_offset = extent.to_offset.as_deref().map(|e| val(vals, e, Kind::Length)).transpose()?.unwrap_or(0.0);
                    let start = level_on(&ss.plane, centre, fp)? + start_offset;
                    let h = level_on(&ss.plane, centre, tp)?;
                    // Past the face by the offset, in the direction travelled.
                    let to = h + if h >= start { to_offset } else { -to_offset };
                    if (to - start).abs() < 1e-9 {
                        return Err(DocError::Invalid("the To object is where the extrude starts".into()));
                    }
                    let moved = |p: &Plane, d: f64| Plane { origin: p.origin + n * d, ..*p };
                    let (from_plane, to_plane) = (moved(fp, start_offset), moved(tp, to - h));
                    // Planes that cross over the profiles would leave out the part of them where
                    // the To plane lies before the From one, chosen by where the middle falls.
                    if planes_cross_over(&from_plane, &to_plane, &ss.plane, &regions, to - start)? {
                        return Err(DocError::Invalid(
                            "the From and To faces' planes cross over the profile, so part of it would have no extent; pick faces that don't cross over it, or use a distance".into(),
                        ));
                    }
                    Some((from_plane, to_plane, to - start))
                }
                _ => None,
            };
            // Whether the extrusion runs toward the To object: false when a To offset carries the
            // end back across the start, so the tool never reaches the object's face.
            let mut toward_to = true;
            let d = match (extent.to, &from_to) {
                (Some(_), Some((_, _, d))) => *d,
                (Some(p), None) => {
                    let off = match &extent.to_offset {
                        Some(e) => val(vals, e, Kind::Length)?,
                        None => 0.0,
                    };
                    let h = height_at(&tst, to_end_face, &ss.plane, centre, p.0);
                    let start = from + extent.start_offset.as_deref().map(|e| val(vals, e, Kind::Length)).transpose()?.unwrap_or(0.0);
                    // Past the face by the offset, in the direction travelled.
                    let to = h + if h >= start { off } else { -off };
                    if (to - start).abs() < 1e-9 {
                        return Err(DocError::Invalid("the To object is where the extrude starts".into()));
                    }
                    toward_to = (h - start) * (to - start) > 0.0;
                    to - start
                }
                (None, _) if extent.through_all => {
                    // The reach is a length; a negative saved distance keeps its side (#10415).
                    // Measured from where the extrude starts, so an offset leaves no side short (#16556).
                    let start = from + extent.start_offset.as_deref().map(|e| val(vals, e, Kind::Length)).transpose()?.unwrap_or(0.0);
                    let reach = through_all_distance(st, &ss.plane, start, &crate::plain(targets))?;
                    if val(vals, &extent.distance, Kind::Length).is_ok_and(|saved| saved < 0.0) { -reach } else { reach }
                }
                (None, _) => val(vals, &extent.distance, Kind::Length)?,
            };
            // The curved (or slanted) To face the tool is cut back to, with a taper or without
            // (#15820): see the straight path below. Not when the extrude runs From a slanted
            // face To a flat one, which ends on the To face's plane.
            // A taper that grows the profile reaches a wall that runs along the extrusion, which
            // the profile itself does not cross: the wall lies within the growth of the profile
            // and trims the tool to the body (#10711). A taper that narrows the profile reaches
            // less of the wall, so it changes nothing.
            let growth = match &extent.taper {
                Some(tp) => {
                    let taper = val(vals, tp, Kind::Angle)?;
                    if taper > 0.0 { d.abs() * taper.tan() } else { 0.0 }
                }
                None => 0.0,
            };
            let curved = curved_face(&tst, to_end_face, Some(n))
                .filter(|_| toward_to && from_to.is_none())
                .filter(|_| !to_end_face.is_some_and(|face| runs_along(&tst, face, n) && !meets_footprint(&tst, face, &ss.plane, &regions, growth)));
            let to_offset = match (&extent.to, &extent.to_offset) {
                (Some(_), Some(e)) => val(vals, e, Kind::Length)?,
                _ => 0.0,
            };
            // A taper of 0 is a straight extrusion (the dialog stores one, and designs carry it);
            // the tapered path refuses a profile with holes without a draft (#13188).
            if let Some(tp) = &extent.taper
                && val(vals, tp, Kind::Angle)? != 0.0
            {
                let taper = val(vals, tp, Kind::Angle)?;
                if extent.distance2.is_some() || extent.direction == crate::Direction::Symmetric {
                    return Err(DocError::Invalid("not supported yet: taper with two-sided or symmetric extents".into()));
                }
                let sign = if extent.to.is_none() && extent.direction == crate::Direction::Negative { -1.0 } else { 1.0 };
                let (len, sign) = if d < 0.0 { (-d, -sign) } else { (d, sign) };
                // Where the extrude starts: the From Object's level and a start offset along the
                // normal from it. The offset moves the profile that far and the sides are drafted
                // from there, so the profile stays the section at the start whatever the offset
                // (`d` is measured from the start already, To Object included).
                let base = from
                    + match &extent.start_offset {
                        Some(e) => val(vals, e, Kind::Length)?,
                        None => 0.0,
                    };
                // To a curved face the tool runs past the face and is cut back to it once built.
                let target = curved.and_then(|bi| tst.bodies.get(bi)).zip(to_face).zip(extent.to);
                let len = match target {
                    Some(((tb, (_, fi)), p)) => tapered_reach(&ss, &regions, (tb, fi, p.0, to_offset), base, sign, len, taper, extent.corner)?,
                    None => len,
                };
                extrude_within_reach([base, base + sign * len])?;
                // A tapered tool is never trimmed: its caps lie at the ends of this interval, as
                // drawn. A tapered cut of a sheet made from surfaces is read by where they lie (see
                // `surface_taper_refused`).
                *constructor = Some(ExtrudeConstructor { plane: ss.plane, interval: (base, base + sign * len) });
                let start_plane = ss.plane.offset(base);
                let caps = [
                    Some(crate::lineage::extrude_cap_role(base)),
                    Some(crate::lineage::extrude_cap_role(if sign < 0.0 { base - len } else { base + len })),
                ];
                // How far a tool grows past its profile, at most: its sides' run over its length.
                let growth = len * taper.tan().abs();
                let mut notes = Vec::new();
                let mut tools: Vec<Traced> = regions
                    .iter()
                    .map(|r| {
                        let roles = crate::lineage::ProfileRoles {
                            sketch: Some(&ss),
                            region: Some(r),
                            caps: caps.clone(),
                            cap_synonyms: [None, None],
                            thin: thin_roles.as_ref(),
                        };
                        let corner = match extent.corner {
                            Some(c) => c,
                            None => draft_corner_for(&ss, r, len, taper, &mut notes)?,
                        };
                        let body = kernel::extrude_tapered_between(&start_plane, r, 0.0, len, 0.0, sign, taper, corner)?;
                        Ok(roles.tool(body, None))
                    })
                    .collect::<Result<_>>()?;
                for note in notes {
                    append_warning(warning, note);
                }
                // Profiles apart by more than the tools grow (the letters of a text): the tools
                // are apart too, and go into the body as one, in one boolean rather than one per
                // profile, each on a body grown or cut by the ones before. (A join meeting several
                // bodies keeps joining them tool by tool.)
                let one_target = if targets.is_empty() { st.bodies.len() == 1 } else { targets.len() == 1 };
                let at_once = match operation {
                    Operation::Cut => true,
                    Operation::Join => one_target,
                    _ => false,
                };
                if at_once && tools.len() > 1 && regions_apart(&regions, APART_GAP + 2.0 * growth) {
                    tools = join_apart_tools(tools)?;
                }
                if let Some(((tb, (_, fi)), p)) = target {
                    tools = trim_to_curved_face(tools, tb, fi, p.0, to_offset, &ss.plane, base, base + sign * len)?;
                }
                return Ok(tools);
            }
            let off = from
                + match &extent.start_offset {
                    Some(e) => val(vals, e, Kind::Length)?,
                    None => 0.0,
                };
            // To Object gives a signed distance from the start: one side only.
            let direction = if extent.to.is_some() { crate::Direction::Positive } else { extent.direction };
            let (a, b) = extent_sides(vals, extent, d, direction)?;
            // A negative distance flips the side.
            let (mut lo, mut hi) = if a <= b { (a, b) } else { (b, a) };
            // To a curved (or slanted) face: run right through the target body, then keep what
            // lies before that face's surface, offset by the To offset along the travel
            // (`trim_to_face`, #12617). An end the offset carries back across the start, away from
            // that face, never meets it: it ends at its level, as for a plane, instead of running
            // out through the whole body. A wall that runs along the extrusion (a cylinder's side,
            // when extruding along its axis) and lies clear of the profile is never reached: the
            // point picked on it stands for the plane through it, as a point in space does
            // (#11923). A wall that crosses the profile seen along the extrusion still trims the
            // tool to the body.
            if let Some(bi) = curved
                && let Some(tb) = tst.bodies.get(bi)
            {
                let m = tb.mesh();
                let mut hs: Vec<f64> = m.positions.iter().map(|q| (*q - ss.plane.origin).dot(n) - off).collect();
                // A flat face at a slant: where its plane, extended past the face, lies over the
                // profiles too, so a profile that overhangs the face still reaches it.
                if let Some(slope) = to_face.and_then(|face| slanted_plane(&tst, face, n)) {
                    let (rise_lo, rise_hi) = slope_rise(&slope, &ss.plane, &regions, off);
                    hs.extend([rise_lo, rise_hi]);
                }
                let margin = tb.body.size() * 0.05 + 1.0 + to_offset.abs();
                if hi > 0.0 {
                    hi = hs.iter().cloned().fold(hi, f64::max) + margin;
                } else {
                    lo = hs.iter().cloned().fold(lo, f64::min) - margin;
                }
            }
            let is_symmetric_offset = direction == crate::Direction::Symmetric && extent.symmetric_offset && off.abs() > 1e-9;
            if is_symmetric_offset && extent.distance2.is_some() {
                return Err(DocError::Invalid("not supported yet: symmetric offset with a second distance".into()));
            }
            if is_symmetric_offset && extent.from.is_some() {
                return Err(DocError::Invalid("not supported yet: symmetric offset with From Object".into()));
            }
            let symmetric_intervals = is_symmetric_offset.then(|| kernel::symmetric_offset_intervals(off, hi));
            // Through all, the tool is the same straight prism, run past the bodies: its interval is
            // where its caps are, so a cut through a sheet made from surfaces goes into the panel's
            // outline with the sketch's own curves (a text's letters as drawn), not read back from
            // the body's.
            let untrimmed = extent.to.is_none() && extent.from.is_none();
            let mut tools = Vec::new();
            if let Some([(pos_lo, pos_hi), (neg_lo, neg_hi)]) = symmetric_intervals {
                extrude_within_reach([neg_lo, pos_hi])?;
                for r in &regions {
                    tools.extend(centered_extrude(
                        &ss.plane,
                        r,
                        pos_lo,
                        pos_hi,
                        Some(&ss),
                        thin_roles.as_ref(),
                        CapRoles::Named(["bottom_pos", "top_pos"]),
                        None,
                    )?);
                    tools.extend(centered_extrude(
                        &ss.plane,
                        r,
                        neg_lo,
                        neg_hi,
                        Some(&ss),
                        thin_roles.as_ref(),
                        CapRoles::Named(["bottom_neg", "top_neg"]),
                        None,
                    )?);
                }
            } else {
                // From a slanted face: the prism runs over the face's whole rise across the profiles,
                // then is cut at the face's plane moved to each end.
                let (run_lo, run_hi) = match (&from_to, &from_slope) {
                    // From a slanted face To a flat one: over both planes' whole rise across the
                    // profiles, then cut at each.
                    (Some((fp, tp, _)), _) => {
                        let (from_lo, from_hi) = slope_rise(fp, &ss.plane, &regions, off);
                        let (to_lo, to_hi) = slope_rise(tp, &ss.plane, &regions, off);
                        let (rise_lo, rise_hi) = (from_lo.min(to_lo), from_hi.max(to_hi));
                        let margin = (rise_hi - rise_lo) * 0.05 + 1.0;
                        (rise_lo - margin, rise_hi + margin)
                    }
                    (None, Some(fp)) => {
                        // The rise is measured over sampled outlines, so the prism runs a margin
                        // past it: its own caps must never reach the result.
                        let (rise_lo, rise_hi) = slope_rise(fp, &ss.plane, &regions, from);
                        let margin = (rise_hi - rise_lo) * 0.05 + 1.0;
                        (lo + rise_lo - margin, hi + rise_hi + margin)
                    }
                    (None, None) => (lo, hi),
                };
                extrude_within_reach([run_lo + off, run_hi + off])?;
                for r in &regions {
                    let cap_roles = if extent.distance2.is_some() { CapRoles::TwoSided } else { CapRoles::ByHeight };
                    tools.extend(centered_extrude(
                        &ss.plane,
                        r,
                        run_lo + off,
                        run_hi + off,
                        Some(&ss),
                        thin_roles.as_ref(),
                        cap_roles,
                        extent.one_sided_direction.filter(|_| direction == crate::Direction::Symmetric && extent.distance2.is_none()),
                    )?);
                }
                if let Some((fp, tp, d)) = &from_to {
                    tools = trim_between_planes(tools, fp, tp, n, d.signum())?;
                } else if let Some(fp) = &from_slope {
                    let shift = off - from;
                    tools = trim_between_slopes(tools, fp, n, lo + shift, hi + shift)?;
                }
            }
            // A cut or a join by profiles apart in the sketch (the letters of a text): each tool
            // lies in the prism over its profile, so the tools are apart too. The body is then cut once by all of them, or joined to
            // all of them at once, rather than once per profile, each on a body grown or cut by
            // the ones before. (A join meeting several bodies keeps joining them tool by tool.)
            let one_target = if targets.is_empty() { st.bodies.len() == 1 } else { targets.len() == 1 };
            let at_once = match operation {
                Operation::Cut => true,
                Operation::Join => one_target,
                _ => false,
            };
            if at_once && tools.len() > 1 && !is_symmetric_offset && regions_apart(&regions, APART_GAP) {
                tools = join_apart_tools(tools)?;
            }
            if let Some(tb) = curved.and_then(|bi| tst.bodies.get(bi))
                && let Some((_, fi)) = to_face
                && let Some(p) = extent.to
            {
                let start = if hi > 0.0 { lo + off } else { hi + off };
                let far = if hi > 0.0 { hi + off } else { lo + off };
                tools = trim_to_curved_face(tools, tb, fi, p.0, to_offset, &ss.plane, start, far)?;
            }
            // A join's profile grows into the material it joins; its sketch plane and depth as drawn
            // still serve a join into sheet metal (`surface_sheet_join`), which takes the sketch's
            // regions.
            if (untrimmed || (*operation == Operation::Join && curved.is_none() && from_slope.is_none())) && !is_symmetric_offset {
                *constructor = Some(ExtrudeConstructor { plane: ss.plane, interval: (lo + off, hi + off) });
            }
            // A Join tool's faces outlive the boolean where they stay on the result, so they are
            // certified as a new body's are (see `Body::with_preserved_union_planar_sources`).
            // A taper of 0 is straight, so it is certified as no taper is (#15413).
            if matches!(operation, Operation::NewBody | Operation::Join) && crate::config::untapered(extent) {
                let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
                let cap_bindings = match thin {
                    Some(thin) => Some(crate::config::thin_extrude_cap_bindings(&thin.curves, regions.len(), &bodies)),
                    None => crate::config::extrude_cap_bindings(&ss, &bodies, 10_000_000),
                };
                let source_curves = cap_bindings.as_ref().map(|_| crate::config::ExactSketchCurves::new(&ss));
                tools = tools
                    .into_iter()
                    .enumerate()
                    .map(|(body_index, Traced { body, lineage, .. })| {
                        let curves: Vec<_> = body
                            .face_sources()
                            .into_iter()
                            .flatten()
                            .map(|source| match source {
                                Some(kernel::FaceSource::ExtrudeSide { segment, .. }) if cap_bindings.is_some() => {
                                    source_curves.as_ref().and_then(|curves| curves.find(segment))
                                }
                                _ => None,
                            })
                            .collect();
                        let mut caps =
                            cap_bindings.as_ref().and_then(|bindings| bindings.get(body_index)).cloned().unwrap_or_else(|| vec![None; curves.len()]);
                        // A cap moved off its designed height (into a Join target, or past a
                        // curved To face) is not the feature's start or end face.
                        let nominal = |plane: &solidcore_record::Plane| {
                            let height = (plane.origin - ss.plane.origin).dot(n);
                            if let Some([(pos_lo, pos_hi), (neg_lo, neg_hi)]) = symmetric_intervals {
                                let tolerance = 1e-9 * (1.0 + height.abs() + pos_hi.abs() + neg_lo.abs());
                                [pos_lo, pos_hi, neg_lo, neg_hi].into_iter().any(|h| (height - h).abs() <= tolerance)
                            } else {
                                let tolerance = 1e-9 * (1.0 + height.abs() + (lo + off).abs() + (hi + off).abs());
                                (height - (lo + off)).abs() <= tolerance || (height - (hi + off)).abs() <= tolerance
                            }
                        };
                        for (cap, source) in caps.iter_mut().zip(body.face_sources().into_iter().flatten()) {
                            if matches!(source, Some(kernel::FaceSource::ExtrudeCap { plane, .. }) if !nominal(plane)) {
                                *cap = None;
                            }
                        }
                        // Certificates set face sources only: the faces, and their lineage, stay.
                        let body = body.with_extrude_certificates(f.id, sketch.0, &curves)?.with_extrude_cap_certificates(f.id, sketch.0, &caps)?;
                        Ok(Traced { body, lineage, recipe: None })
                    })
                    .collect::<Result<Vec<_>>>()?;
            }
            Ok(tools)
        }
        FeatureKind::Revolve { sketch, profiles, axis, angle, angle2, symmetric, convention, to, .. } => {
            let ss = constructor_profile_in_frame(
                doc,
                st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?,
                f,
            )?;
            let regions = solidcraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
            let (o, d) = revolve_axis(
                Some(doc),
                f.component,
                vals,
                &ss,
                axis,
                &*axis_state(doc, st, f.component, axis)?,
                &|name| construction_axis_in(Some(doc), st, name, f.component),
                &|owner, curve| {
                    if owner == sketch.0 {
                        return sketch_line_world(&ss, curve);
                    }
                    let solved =
                        st.sketch(owner).ok_or_else(|| DocError::Unknown(format!("sketch {owner} (it must come earlier in the timeline)")))?;
                    sketch_line_world(&sketch_in_feature_frame(doc, solved, f.component)?, curve)
                },
            )?;
            // To Object is in this feature's component frame: the model as seen from there.
            let tst = if to.is_some() { state_in_frame(doc, st, f.component)? } else { std::borrow::Cow::Borrowed(st) };
            let angles = if to.is_some() {
                (0.0, None)
            } else {
                (val(vals, angle, Kind::Angle)?, angle2.as_ref().map(|a| val(vals, a, Kind::Angle)).transpose()?)
            };
            let extent = revolve_extent_with_convention(
                &tst,
                &ss.plane,
                &regions,
                (ss.plane.to_world(o), ss.plane.dir_to_world(d)),
                angles,
                *symmetric,
                to.map(Vec3::from),
                *convention,
            )?;
            let (start, ang) = (extent.start, extent.sweep);
            let region_extents = regions
                .iter()
                .map(|r| {
                    if *convention == crate::RevolveConvention::SketchNormal && to.is_some() {
                        revolve_extent_with_convention(
                            &tst,
                            &ss.plane,
                            std::slice::from_ref(r),
                            (ss.plane.to_world(o), ss.plane.dir_to_world(d)),
                            angles,
                            *symmetric,
                            to.map(Vec3::from),
                            *convention,
                        )
                    } else {
                        Ok(extent)
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let (operation, _) = feature_op(f);
            let axis_w = ss.plane.dir_to_world(d);
            let origin_w = ss.plane.to_world(o);
            // To a curved face: turn past the whole target body, then cut back to its surface
            // (the piece that starts at the profile).
            let curved = curved_face(&tst, to.and_then(|p| object_face(&tst, f.face_names.first().map(String::as_str), p.0, None)), None);
            let curved_body = curved.and_then(|bi| tst.bodies.get(bi));
            // Each region's body: its sides named by the sketch curves, its caps start and end.
            let tool = |r: &Region2, body: Body, built: Option<&Body>| crate::lineage::ProfileRoles::swept(Some(&ss), Some(r)).tool(body, built);
            let build_curved = |regions: &[Region2], angle_axis: Vec3, tb: &ModelBody| -> Result<Vec<Traced>> {
                let pts: Vec<Vec2> = regions.iter().flat_map(|r| r.outer.polyline(1e-2)).collect();
                let c = ss.plane.to_world(pts.iter().fold(Vec2::default(), |a, q| a + *q) * (1.0 / pts.len().max(1) as f64));
                let flat = |q: Vec3| {
                    let v = q - origin_w;
                    v - axis_w * v.dot(axis_w)
                };
                let u0 = flat(c);
                let angle_of = |q: Vec3| {
                    let v = flat(q);
                    let a = angle_axis.dot(u0.cross(v)).atan2(u0.dot(v));
                    if a < 0.0 { a + std::f64::consts::TAU } else { a }
                };
                let far = tb.mesh().positions.iter().map(|q| angle_of(*q)).fold(0.0, f64::max);
                let sweep = (far + 0.05).min(std::f64::consts::TAU - 1e-3);
                let bodies = kernel::revolve(&ss.plane, regions, o, d, sweep)?;
                let tools = regions.iter().zip(bodies).map(|(r, body)| tool(r, body, None)).collect();
                let n = ss.plane.normal();
                let tol_side = |q: Vec3| if flat(q).dot(u0) > 0.0 { ((q - ss.plane.origin).dot(n)).abs() } else { f64::MAX };
                trim_to_body(tools, tb, tol_side)
            };
            if *convention == crate::RevolveConvention::LegacyAxisStart
                && let Some(tb) = curved_body
            {
                return build_curved(&regions, -axis_w, tb);
            }
            // A new body is final: its planar faces come out whole. A boolean's tool is not
            // healed (the boolean heals its result).
            let revolve = if operation == Operation::NewBody { kernel::revolve_whole } else { kernel::revolve };
            if *convention == crate::RevolveConvention::SketchNormal {
                let mut out = Vec::new();
                for (region, region_extent) in regions.iter().zip(&region_extents) {
                    let rotation_axis = kernel::canonical_revolve_axis(&ss.plane, region, o, d)?;
                    if let Some(tb) = curved_body {
                        out.extend(build_curved(std::slice::from_ref(region), rotation_axis, tb)?);
                        continue;
                    }
                    let bodies = revolve(&ss.plane, std::slice::from_ref(region), o, d, region_extent.sweep)?;
                    let turn = region_extent.start;
                    let m = crate::rigid(Vec3::ZERO, origin_w, rotation_axis, turn);
                    // A whole turn's tool has the seam of its surface of revolution in the profile
                    // plane. A body that holds a seam in that plane (a body revolved in it) makes
                    // SMLib's boolean split both and leave an edge near the seam, which its check
                    // refuses (#18039): the tool's seam is turned away from the plane then, the
                    // solid being the same.
                    let turn_it = operation != Operation::NewBody
                        && (region_extent.sweep.abs() - std::f64::consts::TAU).abs() <= 1e-9
                        && bodies_hold_a_seam_in_plane(st, f, &ss.plane);
                    for body in bodies {
                        let traced = if turn.abs() < 1e-12 {
                            tool(region, body, None)
                        } else {
                            let moved = kernel::transform_matrix(&body, m)?;
                            tool(region, moved, Some(&body))
                        };
                        // The tool's faces take their roles from the revolve's history before the
                        // seam is turned: the turned body's own history is the turn's, which says
                        // nothing of the profile, and read as the revolve's it named the faces after
                        // the wrong sketch curves (the axis line). The turn keeps the faces' order.
                        out.push(if turn_it {
                            Traced { body: turned_seam(rotation_axis, origin_w, &traced.body)?, lineage: traced.lineage, recipe: traced.recipe }
                        } else {
                            traced
                        });
                    }
                }
                return Ok(out);
            }
            let bodies = revolve(&ss.plane, &regions, o, d, ang)?;
            let turn = -start;
            let turn_it =
                operation != Operation::NewBody && (ang.abs() - std::f64::consts::TAU).abs() <= 1e-9 && bodies_hold_a_seam_in_plane(st, f, &ss.plane);
            let tools: Vec<Traced> = if turn.abs() < 1e-12 {
                regions.iter().zip(bodies).map(|(r, body)| tool(r, body, None)).collect()
            } else {
                // Turned back to start before the profile.
                let m = crate::rigid(Vec3::ZERO, origin_w, axis_w, turn);
                regions
                    .iter()
                    .zip(bodies)
                    .map(|(r, body)| Ok(tool(r, kernel::transform_matrix(&body, m)?, Some(&body))))
                    .collect::<Result<Vec<_>>>()?
            };
            tools
                .into_iter()
                .map(|t| {
                    let body = if turn_it { turned_seam(axis_w, origin_w, &t.body)? } else { t.body.clone() };
                    Ok(Traced { body, lineage: t.lineage, recipe: t.recipe })
                })
                .collect()
        }
        FeatureKind::Loft { sections, rails, rail_fit, .. } if sections.iter().any(|s| s.point.is_some()) => {
            // A profile lofted to a sketch point (an apex), following its guide rails if it has any.
            let [a, b] = &sections[..] else { return Err(DocError::Invalid("not supported yet: a point section in a loft of more than two".into())) };
            let (prof, pt) = if a.point.is_some() { (b, a) } else { (a, b) };
            let ss = st.sketch(prof.sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {}", prof.sketch)))?;
            let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, &prof.profiles)?);
            let [r] = &regions[..] else { return Err(DocError::Invalid("each loft section must be one profile".into())) };
            refuse_loft_holes(r)?;
            let ps = st.sketch(pt.sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {}", pt.sketch)))?;
            let id = pt.point.as_deref().unwrap_or_default();
            let q = ps.sketch.points.iter().find(|p| p.id == id).ok_or_else(|| DocError::Unknown(format!("point `{id}` in {}", ps.name)))?;
            let apex = ps.plane.to_world(q.pos);
            let body = if rails.is_empty() {
                kernel::loft_to_point(&ss.plane, &r.outer, apex)?
            } else {
                // The rails run from the end at the section.
                let start = r.outer.segs.first().map(|seg| ss.plane.to_world(seg.start())).unwrap_or_default();
                kernel::loft_to_point_with_rails(
                    &ss.plane,
                    &r.outer,
                    apex,
                    &rail_polylines(doc, st, rails, f.component, start)?,
                    kernel_rail_fit(*rail_fit),
                )?
            };
            // Its sides are named by the profile's curves, its base `start`.
            Ok(vec![crate::lineage::ProfileRoles::swept(Some(ss), Some(r)).tool(body, None)])
        }
        FeatureKind::Coil {
            base,
            axis,
            diameter,
            pitch,
            turns,
            section_size,
            section,
            position,
            start_angle,
            clockwise,
            end_cut,
            sketch,
            profiles,
            ..
        } => {
            let base = &point(vals, base)?;
            let k = axis.normalized().ok_or_else(|| DocError::Invalid("coil axis".into()))?;
            let p = val(vals, pitch, Kind::Length)?;
            let n = val(vals, turns, Kind::Unitless)?;
            if sketch.is_some()
                && (diameter.is_some()
                    || section_size.is_some()
                    || start_angle.is_some()
                    || !matches!(section, crate::CoilSection::Circular)
                    || *position != 0
                    || !matches!(end_cut, crate::CoilEndCut::Axial))
            {
                return Err(DocError::Invalid(
                    "a coil with a `sketch` cannot also set `diameter`, `section_size`, `section`, `section_position`, `start_angle` or `end_cut`"
                        .into(),
                ));
            }
            if sketch.is_none() && !matches!(profiles, ProfileSel::All) {
                return Err(DocError::Invalid("a coil's `profiles` need a `sketch`".into()));
            }
            if let Some(sk) = sketch {
                let ss = st.sketch(sk.0).ok_or_else(|| DocError::Unknown(format!("sketch {sk} (it must come earlier in the timeline)")))?;
                let regions = select_profiles(ss, profiles)?;
                if regions.iter().any(|r| !r.holes.is_empty()) {
                    return Err(DocError::Invalid("a coil from a sketch takes a profile without holes".into()));
                }
                let [region] = &regions[..] else {
                    return Err(DocError::Invalid(format!("a coil from a sketch takes one closed profile (the sketch has {})", regions.len())));
                };
                let axis = if *clockwise { k * -1.0 } else { k };
                // A left-handed coil turns about −axis and climbs along +axis: mirror the pitch.
                let pitch = if *clockwise { -p } else { p };
                return Ok(unnamed(vec![kernel::sweep_helix(&ss.plane, region, *base, axis, pitch, n)?]));
            }
            let diameter = diameter.as_ref().ok_or_else(|| DocError::Invalid("a coil needs a `sketch` or a `diameter`".into()))?;
            let section_size = section_size.as_ref().ok_or_else(|| DocError::Invalid("a coil needs a `sketch` or a `section_size`".into()))?;
            let (d, s) = (val(vals, diameter, Kind::Length)?, val(vals, section_size, Kind::Length)?);
            if !(d > 0.0 && s > 0.0 && n > 0.0) {
                return Err(DocError::Invalid("the coil diameter, section size and revolutions must be positive".into()));
            }
            let r = d / 2.0 + f64::from(*position) * s / 2.0;
            if r <= s / 2.0 {
                return Err(DocError::Invalid("the section reaches the coil's axis".into()));
            }
            let a0 = match start_angle {
                Some(e) => val(vals, e, Kind::Angle)?,
                None => 0.0,
            };
            // Radial direction at the start; the section lies in the plane of the axis.
            let x0 = if k.cross(Vec3::X).len() > 1e-6 { (Vec3::X - k * k.dot(Vec3::X)).normalized() } else { Some(k.any_perp()) };
            let x0 = x0.ok_or_else(|| DocError::Invalid("coil axis".into()))?;
            let x = x0 * a0.cos() + k.cross(x0) * a0.sin();
            let axis = if *clockwise { k * -1.0 } else { k };
            // A left-handed coil turns about −axis and climbs along +axis: mirror the pitch.
            let pitch = if *clockwise { -p } else { p };
            // Square ends: the section lies across the helix's tangent at the start, and the screw
            // motion keeps it so, out to the other end.
            let y = match end_cut {
                crate::CoilEndCut::Axial => k,
                crate::CoilEndCut::Square => {
                    let t = (axis.cross(x) * (std::f64::consts::TAU * r) + axis * pitch).normalized();
                    t.and_then(|t| (k - t * k.dot(t)).normalized()).ok_or_else(|| DocError::Invalid("coil axis".into()))?
                }
            };
            let plane = Plane { origin: *base, x, y };
            let c = Vec2::new(r, 0.0);
            let outer = match section {
                crate::CoilSection::Circular => solidcore_record::Loop2::circle(c, s / 2.0),
                crate::CoilSection::Square => {
                    let h = s / 2.0;
                    solidcore_record::Loop2::polygon(&[c + Vec2::new(-h, -h), c + Vec2::new(h, -h), c + Vec2::new(h, h), c + Vec2::new(-h, h)])
                }
            };
            let region = Region2 { outer, holes: Vec::new() };
            let coil = Traced::unnamed(kernel::sweep_helix(&plane, &region, *base, axis, pitch, n)?);
            Ok(vec![coil.with_recipe(tool_recipe(doc, f, vals))])
        }
        FeatureKind::Loft { sections, rails, rail_fit, .. } => {
            let mut secs = Vec::new();
            let mut sketches = Vec::new();
            for sec in sections {
                let ss = st
                    .sketch(sec.sketch.0)
                    .ok_or_else(|| DocError::Unknown(format!("sketch {} (it must come earlier in the timeline)", sec.sketch)))?;
                let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, &sec.profiles)?);
                let [r] = &regions[..] else { return Err(DocError::Invalid("each loft section must be one profile".into())) };
                secs.push((ss.plane, r.clone()));
                sketches.push(ss);
            }
            // Its caps are the first and last sections, its sides named by the curves they run
            // between.
            let body = if rails.is_empty() {
                kernel::loft_regions(&secs)?
            } else {
                // Rails shape the sections' outlines; sections with holes wait for rails that
                // carry the holes too.
                if secs.iter().any(|(_, r)| !r.holes.is_empty()) {
                    return Err(DocError::Invalid("not supported yet: guide rails on a loft whose sections have holes".into()));
                }
                let outlines: Vec<_> = secs.iter().map(|(p, r)| (*p, r.outer.clone())).collect();
                // The rails run from the end at the first section.
                let start = outlines.first().and_then(|(p, lp)| lp.segs.first().map(|seg| p.to_world(seg.start()))).unwrap_or_default();
                kernel::loft_with_rails(&outlines, &rail_polylines(doc, st, rails, f.component, start)?, kernel_rail_fit(*rail_fit))?
            };
            let roles = crate::lineage::SectionRoles {
                sections: sketches.into_iter().zip(secs.iter().map(|(_, r)| std::iter::once(&r.outer).chain(&r.holes).collect())).collect(),
            };
            Ok(vec![roles.tool(body)])
        }
        FeatureKind::Sweep { sketch, profiles, path_sketch, path, path_body, path_edges, orientation, twist, scale, rails, rail_fit, .. } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?;
            // The path is a chain of a body's edges (#2216) or the curves of a sketch.
            if let Some(body) = path_body {
                let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, profiles)?);
                let start = regions.first().map(|r| ss.plane.to_world(r.centroid())).unwrap_or_default();
                let tool = |r: &Region2, body: Body| crate::lineage::ProfileRoles::swept(Some(ss), Some(r)).tool(body, None);
                let segs = edge_path_segments(st, body, path_edges, start, Some(&ss.plane))?;
                let path_start = segs.first().map(kernel::PathSeg::start).unwrap_or(start);
                let rails = rail_polylines(doc, st, rails, f.component, path_start)?;
                let ctl = sweep_control(vals, orientation, twist, scale, rails, *rail_fit)?;
                return regions.iter().map(|r| Ok(tool(r, kernel::sweep_with(&ss.plane, r, &segs, &ctl)?))).collect();
            }
            // Each path curve in the sketch it is in, mapped into the feature's frame: the path
            // may join the curves of several sketches (#16168). The profile and the path are read
            // through the frame captured when the sweep was made, so the body it builds moves with
            // the sweep's own component; a sweep that reads components of its own keeps the live
            // placement of each. A guide rail is read through its own live placement, so a sweep
            // that has one keeps that too.
            let readers = sweep_readers(doc, sketch.0, path, *path_sketch, rails);
            let frames = ReadFrames::of(doc, f, &readers, rails.is_empty());
            let curves = path_curves(doc, st, path, *path_sketch, f.component, &frames)?;
            // The profile joins the path in the feature's frame, whatever sketch each is drawn
            // in (identity when a sketch shares the feature's component).
            let profile_owner = doc.feature(sketch.0).map(|g| g.component).unwrap_or(0);
            let profile = profile_with_transform(ss, &frames.relative(doc, profile_owner)?)?;
            let ss = &profile;
            let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, profiles)?);
            let start = regions.first().map(|r| ss.plane.to_world(r.centroid())).unwrap_or_default();
            let tool = |r: &Region2, body: Body| crate::lineage::ProfileRoles::swept(Some(ss), Some(r)).tool(body, None);
            let points = path_points(&curves, start)?;
            let segs = match points {
                Some(_) => Vec::new(),
                None => path_segments_at(&curves, start, Some(&ss.plane))?,
            };
            let path_start =
                points.as_ref().and_then(|p| p.points().first().copied()).or_else(|| segs.first().map(kernel::PathSeg::start)).unwrap_or(start);
            let rails = rail_polylines(doc, st, rails, f.component, path_start)?;
            let ctl = sweep_control(vals, orientation, twist, scale, rails, *rail_fit)?;
            // A path through 3D sketch curves: its drawn splines exactly, the rest as points.
            if let Some(path) = points {
                return regions.iter().map(|r| Ok(tool(r, kernel::sweep_point_path_with(&ss.plane, r, &path, &ctl)?))).collect();
            }
            regions.iter().map(|r| Ok(tool(r, kernel::sweep_with(&ss.plane, r, &segs, &ctl)?))).collect()
        }
        FeatureKind::Pipe { path_sketch, path, diameter, wall, .. } => {
            let ps =
                st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
            let mapped = path_in_frame(doc, ps, f.component)?;
            let ps = mapped.as_ref();
            let r = val(vals, diameter, Kind::Length)? / 2.0;
            if !(r > 1e-6) {
                return Err(DocError::Invalid("pipe diameter must be positive".into()));
            }
            let region = pipe_region(vals, r, wall)?;
            // Its faces are named as a sweep's caps and a cylinder's wall are (#14897).
            let tool = |body: Body| crate::lineage::pipe_tool(body, region.outer.segs.len());
            if let Some(along) = path_points(&curves_in(ps, path), Vec3::new(f64::NAN, f64::NAN, f64::NAN))? {
                let pts = along.points();
                let (Some(a), Some(b)) = (pts.first().copied(), pts.get(1).copied()) else { return Err(DocError::Invalid("empty path".into())) };
                let t = (b - a).normalized().ok_or_else(|| DocError::Invalid("path direction".into()))?;
                let plane = Plane::from_normal(a, t).ok_or_else(|| DocError::Invalid("pipe profile plane".into()))?;
                return Ok(vec![tool(kernel::sweep_point_path_with(&plane, &region, &along, &kernel::SweepControl::default())?)]);
            }
            let segs = path_segments(ps, path, Vec3::new(f64::NAN, f64::NAN, f64::NAN))?;
            let first = segs.first().ok_or_else(|| DocError::Invalid("empty path".into()))?;
            let start = first.start();
            let (t, _) = first.tangents().ok_or_else(|| DocError::Invalid("path direction".into()))?;
            let plane = Plane::from_normal(start, t).ok_or_else(|| DocError::Invalid("pipe profile plane".into()))?;
            Ok(vec![tool(kernel::sweep(&plane, &region, &segs)?)])
        }
        FeatureKind::Box { corner, length, width, height, centred, .. } => {
            let s = Vec3::new(val(vals, length, Kind::Length)?, val(vals, width, Kind::Length)?, val(vals, height, Kind::Length)?);
            let corner = box_low_corner(point(vals, corner)?, s, *centred);
            Ok(vec![crate::lineage::box_tool(kernel::box_solid(corner, corner + s)?).with_recipe(tool_recipe(doc, f, vals))])
        }
        FeatureKind::Cylinder { base, axis, radius, height, .. } => {
            let axis = direction(vals, axis)?;
            Ok(vec![
                crate::lineage::cylinder_tool(kernel::cylinder(
                    cylinder_base(vals, st, f, base, axis)?,
                    axis,
                    val(vals, radius, Kind::Length)?,
                    val(vals, height, Kind::Length)?,
                )?)
                .with_recipe(tool_recipe(doc, f, vals)),
            ])
        }
        FeatureKind::Sphere { center, radius, .. } => Ok(unnamed(vec![kernel::sphere(point(vals, center)?, val(vals, radius, Kind::Length)?)?])),
        FeatureKind::Torus { center, major, minor, .. } => {
            Ok(unnamed(vec![kernel::torus(point(vals, center)?, val(vals, major, Kind::Length)?, val(vals, minor, Kind::Length)?)?]))
        }
        FeatureKind::Cone { base, axis, radius, top_radius, height, .. } => Ok(vec![more::cone(
            point(vals, base)?,
            axis.0,
            val(vals, radius, Kind::Length)?,
            val(vals, top_radius, Kind::Length)?,
            val(vals, height, Kind::Length)?,
        )?]),
        FeatureKind::Hole { .. } => Ok(hole_feature_tools(doc, vals, st, f)?.into_iter().map(|tool| tool.traced).collect()),
        _ => Err(DocError::Invalid(format!("{} cannot be patterned or mirrored yet", f.name))),
    }
}

/// A chain of sketch curves as 3D path segments, starting at the end nearest `start`.
/// A sweep's options from its stored orientation, its twist and end scale expressions (none: no
/// twist, scale 1) and its guide rails as polylines (see [`rail_polylines`]). Out-of-range values
/// are refused by the kernel.
pub(crate) fn sweep_control(
    vals: &BTreeMap<String, Value>,
    orientation: &crate::SweepOrientation,
    twist: &Option<crate::Expr>,
    scale: &Option<crate::Expr>,
    rails: Vec<Vec<Vec3>>,
    rail_fit: crate::RailFit,
) -> Result<kernel::SweepControl> {
    let orientation = match *orientation {
        crate::SweepOrientation::Perpendicular => kernel::SweepOrientation::Perpendicular,
        crate::SweepOrientation::Parallel => kernel::SweepOrientation::Parallel,
        crate::SweepOrientation::Direction(d) => kernel::SweepOrientation::Direction(d.0),
    };
    let twist = match twist {
        Some(t) => val(vals, t, Kind::Angle)?,
        None => 0.0,
    };
    let scale = match scale {
        Some(k) => val(vals, k, Kind::Unitless)?,
        None => 1.0,
    };
    let ctl = kernel::SweepControl { orientation, twist, scale, rails, rail_fit: kernel_rail_fit(rail_fit) };
    ctl.check()?;
    Ok(ctl)
}

/// How guide rails reshape a sweep's profile or a loft's sections, for the kernel.
pub(crate) fn kernel_rail_fit(fit: crate::RailFit) -> kernel::RailFit {
    match fit {
        crate::RailFit::Scale => kernel::RailFit::Scale,
        crate::RailFit::Stretch => kernel::RailFit::Stretch,
    }
}

/// A sweep's or a loft's guide rails as polylines in the feature's frame, each a chain of its sketch's curves
/// (planar or 3D) from the end nearest `start` (the start of the path, the loft's first section).
pub(crate) fn rail_polylines(doc: &Document, st: &ModelState, rails: &[crate::SweepRail], consumer: u64, start: Vec3) -> Result<Vec<Vec<Vec3>>> {
    if rails.len() > 2 {
        return Err(DocError::Invalid("a sweep or a loft takes one or two guide rails".into()));
    }
    rails
        .iter()
        .map(|rail| {
            if rail.curves.is_empty() {
                return Err(DocError::Invalid("a guide rail needs its curves".into()));
            }
            let rs = st
                .sketch(rail.sketch.0)
                .ok_or_else(|| DocError::Unknown(format!("rail sketch {} (it must come earlier in the timeline)", rail.sketch)))?;
            let mapped = path_in_frame(doc, rs, consumer)?;
            let rs = mapped.as_ref();
            match path_points(&curves_in(rs, &rail.curves), start)? {
                Some(path) => Ok(path.points()),
                None => Ok(kernel::path_polyline(&path_segments(rs, &rail.curves, start)?)?),
            }
        })
        .collect()
}

/// A pipe's section: a circle of radius `r`, hollow when it has a wall.
fn pipe_region(vals: &BTreeMap<String, Value>, r: f64, wall: &Option<crate::Expr>) -> Result<Region2> {
    let holes = match wall {
        Some(w) => {
            let wt = val(vals, w, Kind::Length)?;
            if !(wt > 0.0 && wt < r) {
                return Err(DocError::Invalid("the wall must be thinner than the radius".into()));
            }
            vec![solidcore_record::Loop2::circle(Vec2::ZERO, r - wt).reversed()]
        }
        None => Vec::new(),
    };
    Ok(Region2 { outer: solidcore_record::Loop2::circle(Vec2::ZERO, r), holes })
}

pub fn map_wire(wire: &mut solidcraft_sketch::Wire, matrix: &Mat) {
    wire.map(|p| crate::apply_point(matrix, p), |v| crate::apply_vector(matrix, v));
}

/// A feature's sketch as its component sees it: a sketch of another component that is placed
/// differently is moved through both placements (as `state_in_frame` does for sketches on faces),
/// so the tool is built where the sketch is shown. The sketch itself when they share a frame.
fn sketch_in_feature_frame(doc: &Document, sketch: &SolvedSketch, consumer: u64) -> Result<SolvedSketch> {
    let owner = doc.feature(sketch.feature).map(|feature| feature.component).unwrap_or(0);
    if owner == consumer || doc.occurrences.is_empty() {
        return Ok(sketch.clone());
    }
    profile_in_frame(doc, sketch, consumer)
}

fn constructor_profile_in_frame(doc: &Document, sketch: &SolvedSketch, feature: &Feature) -> Result<SolvedSketch> {
    if let Some(consumer_in_lca) = feature.profile_frame {
        if !crate::is_placement(&consumer_in_lca) {
            return Err(DocError::Invalid("stored profile frame is invalid".into()));
        }
        let owner = doc.feature(sketch.feature).map(|f| f.component).unwrap_or(0);
        let lca = doc.common_ancestor(feature.component, owner);
        let owner_in_lca = doc.component_transform_in(owner, lca);
        let inverse = crate::mat_inverse(&consumer_in_lca).ok_or_else(|| DocError::Invalid("stored profile frame is singular".into()))?;
        let relative = crate::mat_mul(&inverse, &owner_in_lca);
        return profile_with_transform(sketch, &relative);
    }
    match crate::body_transfer::profile_frame(doc, feature, sketch.feature) {
        Some(relative) => profile_with_transform(sketch, &relative),
        None => sketch_in_feature_frame(doc, sketch, feature.component),
    }
}

fn profile_in_frame(doc: &Document, sketch: &SolvedSketch, consumer: u64) -> Result<SolvedSketch> {
    let owner = doc.feature(sketch.feature).map(|feature| feature.component).unwrap_or(0);
    let inverse =
        crate::mat_inverse(&doc.component_transform(consumer)).ok_or_else(|| DocError::Invalid("profile consumer frame is invalid".into()))?;
    let relative = crate::mat_mul(&inverse, &doc.component_transform(owner));
    profile_with_transform(sketch, &relative)
}

/// The frames a feature reads the geometry of other components through: the frame it captured
/// when it was made (its own component as it stood then, against the common ancestor of the two
/// components), so that the body it builds moves with its own component, or the live placement of
/// the component the geometry is in.
struct ReadFrames {
    /// The captured frame and the common ancestor it was captured against.
    captured: Option<(Mat, u64)>,
    /// The feature's own component.
    consumer: u64,
}

impl ReadFrames {
    /// The frames a feature reads the components it names through. The captured frame applies
    /// when every one of them shares the common ancestor it was captured against; else each is
    /// read through its live placement, as before.
    fn of(doc: &Document, feature: &Feature, owners: &[u64], use_captured: bool) -> Self {
        let profile_owner = owners.first().copied().unwrap_or(0);
        let ancestor = doc.common_ancestor(feature.component, profile_owner);
        let frame = if use_captured { feature.profile_frame } else { None };
        let captured = frame.filter(|_| owners.iter().all(|o| doc.common_ancestor(feature.component, *o) == ancestor));
        Self { captured: captured.map(|frame| (frame, ancestor)), consumer: feature.component }
    }

    /// The frame of `owner` in the feature's own component's frame.
    fn relative(&self, doc: &Document, owner: u64) -> Result<Mat> {
        match self.captured {
            Some((consumer_in_lca, ancestor)) => {
                let owner_in_lca = doc.component_transform_in(owner, ancestor);
                let inverse = crate::mat_inverse(&consumer_in_lca).ok_or_else(|| DocError::Invalid("stored profile frame is singular".into()))?;
                Ok(crate::mat_mul(&inverse, &owner_in_lca))
            }
            None => {
                let inverse = crate::mat_inverse(&doc.component_transform(self.consumer))
                    .ok_or_else(|| DocError::Invalid("consumer frame is invalid".into()))?;
                Ok(crate::mat_mul(&inverse, &doc.component_transform(owner)))
            }
        }
    }
}

fn profile_with_transform(sketch: &SolvedSketch, relative: &Mat) -> Result<SolvedSketch> {
    let mut mapped = sketch.clone();
    mapped.plane = Plane::new(
        crate::apply_point(relative, sketch.plane.origin),
        crate::apply_vector(relative, sketch.plane.x),
        crate::apply_vector(relative, sketch.plane.y),
    )
    .ok_or_else(|| DocError::Invalid("profile sketch frame is invalid".into()))?;
    Ok(mapped)
}

/// Stored coordinates are converted once into the feature consumer frame: the sketch's plane
/// (its planar curves are read through it, #16168) and its 3D wires.
fn path_in_frame<'a>(doc: &Document, sketch: &'a SolvedSketch, consumer: u64) -> Result<std::borrow::Cow<'a, SolvedSketch>> {
    let owner = doc.feature(sketch.feature).map(|f| f.component).unwrap_or(0);
    let inverse =
        crate::mat_inverse(&doc.component_transform(consumer)).ok_or_else(|| DocError::Invalid("path consumer frame is not invertible".into()))?;
    let relative = crate::mat_mul(&inverse, &doc.component_transform(owner));
    path_with_frame(sketch, &relative, &inverse)
}

/// A sketch's plane and its 3D wires in the frame they are read in: the relative frame of the
/// sketch's component in the consumer's, and the consumer's own frame inverted for the wires
/// that are in the world.
fn path_with_frame<'a>(sketch: &'a SolvedSketch, relative: &Mat, inverse: &Mat) -> Result<std::borrow::Cow<'a, SolvedSketch>> {
    use std::borrow::Cow;
    let mut mapped = sketch.clone();
    mapped.plane = Plane::new(
        crate::apply_point(relative, sketch.plane.origin),
        crate::apply_vector(relative, sketch.plane.x),
        crate::apply_vector(relative, sketch.plane.y),
    )
    .ok_or_else(|| DocError::Invalid("path sketch frame is invalid".into()))?;
    for wire in &mut mapped.sketch.wires {
        let matrix = if wire.space == solidcraft_sketch::WireSpace::Component { relative } else { inverse };
        map_wire(wire, matrix);
    }
    Ok(Cow::Owned(mapped))
}

/// One curve of a path: the sketch it is in (already in the feature's frame) and the curve's id.
/// A path may join the curves of several sketches (#16168); the sketches are shared, one each.
pub(crate) struct PathCurve {
    ss: std::rc::Rc<SolvedSketch>,
    id: String,
}

/// Every id as a curve of the one sketch `ps`, for the path helpers' callers whose path is the
/// curves of a single sketch.
fn curves_in(ps: &SolvedSketch, ids: &[String]) -> Vec<PathCurve> {
    let ss = std::rc::Rc::new(ps.clone());
    ids.iter().map(|id| PathCurve { ss: ss.clone(), id: id.clone() }).collect()
}

/// A sweep's path curves resolved: each entry's curve in its own sketch, each sketch mapped into
/// the feature's frame (#16168). An entry without a sketch of its own is a curve of the path's
/// sketch, as designs before cross-sketch paths saved.
fn path_curves(
    doc: &Document,
    st: &ModelState,
    path: &[crate::PathEntry],
    path_sketch: crate::SketchId,
    component: u64,
    frames: &ReadFrames,
) -> Result<Vec<PathCurve>> {
    let mut mapped: Vec<(u64, std::rc::Rc<SolvedSketch>)> = Vec::new();
    // The wires that are in the world stay in the world: they follow what they are linked to.
    let inverse =
        crate::mat_inverse(&doc.component_transform(component)).ok_or_else(|| DocError::Invalid("path consumer frame is not invertible".into()))?;
    for entry in path {
        let (sid, _) = entry.split(path_sketch);
        if !mapped.iter().any(|(s, _)| *s == sid.0) {
            let sk = st.sketch(sid.0).ok_or_else(|| DocError::Unknown(format!("sketch {sid} (it must come earlier in the timeline)")))?;
            let owner = doc.feature(sid.0).map(|f| f.component).unwrap_or(0);
            let sketch = std::rc::Rc::new(path_with_frame(sk, &frames.relative(doc, owner)?, &inverse)?.into_owned());
            mapped.push((sid.0, sketch));
        }
    }
    let mut curves: Vec<PathCurve> = Vec::with_capacity(path.len());
    for entry in path {
        let (sid, id) = entry.split(path_sketch);
        let (_, ss) = mapped.iter().find(|(s, _)| *s == sid.0).ok_or_else(|| DocError::Unknown(format!("sketch {sid}")))?;
        curves.push(PathCurve { ss: ss.clone(), id: id.to_string() });
    }
    Ok(curves)
}

/// The components a sweep reads its profile, its path and its rails from.
fn sweep_readers(doc: &Document, profile: u64, path: &[crate::PathEntry], path_sketch: crate::SketchId, rails: &[crate::SweepRail]) -> Vec<u64> {
    let owner = |sketch: u64| doc.feature(sketch).map(|f| f.component).unwrap_or(0);
    let mut owners = vec![owner(profile)];
    for entry in path {
        owners.push(owner(entry.split(path_sketch).0.0));
    }
    for rail in rails {
        owners.push(owner(rail.sketch.0));
    }
    owners
}

/// A path that uses 3D sketch curves (wires), as one chain of pieces put in order from the end
/// nearest `start` (no start: the first curve runs toward the second): each drawn 3D spline as
/// its fit points (the kernel has its exact curve), each curve held exactly (a planar sketch
/// curve, a drawn 3D control point spline, a linked curve with
/// [`solidcraft_sketch::Wire::exact`]) as its path segments, and a
/// linked curve known only as points as those points. `None` when the path is all planar curves.
fn path_points(curves: &[PathCurve], start: Vec3) -> Result<Option<kernel::PointPath>> {
    if !curves.iter().any(|c| c.ss.sketch.wire_index(&c.id).is_some()) {
        return Ok(None);
    }
    if curves.is_empty() || curves.len() > 1000 {
        return Err(DocError::Invalid("the path needs 1…1000 curves".into()));
    }
    let mut pieces: Vec<kernel::PathPiece> = Vec::with_capacity(curves.len());
    for c in curves {
        if let Some(wi) = c.ss.sketch.wire_index(&c.id) {
            pieces.push(match c.ss.sketch.wires.get(wi) {
                // A drawn 3D arc is its circle, from its start counter-clockwise about its normal
                // (its three fit points define it; a spline through them is not the arc).
                Some(solidcraft_sketch::Wire { arc: Some(arc), .. }) => {
                    let x = (arc.start - arc.center).normalized().ok_or_else(|| DocError::Invalid("a 3D arc of no radius".into()))?;
                    let y = arc.normal.cross(x);
                    let frame = solidcore_record::brep::Frame { origin: arc.center.into(), x_axis: x.into(), y_axis: y.into() };
                    let circle = solidcore_record::brep::Curve::Circle { frame, radius: arc.radius };
                    kernel::PathPiece::Segs(kernel::curve_path(&circle, [0.0, solidcraft_sketch::arc_sweep(arc)])?)
                }
                // A drawn control point spline is its B-spline, as its Bézier spans (a spline
                // through its control points is another curve).
                Some(w) if w.control.is_some() => {
                    let c = w.control_curve().ok_or_else(|| DocError::Invalid(format!("3D control point spline `{}` has no curve", w.id)))?;
                    kernel::PathPiece::Segs(kernel::curve_path(&c.curve, c.interval)?)
                }
                Some(w) if w.fit.len() >= 3 => kernel::PathPiece::Spline { fit: w.fit.clone(), reversed: false },
                Some(w) if !w.exact.is_empty() => {
                    let mut segs = Vec::new();
                    for c in &w.exact {
                        segs.extend(kernel::curve_path(&c.curve, c.interval)?);
                    }
                    kernel::PathPiece::Segs(segs)
                }
                w => kernel::PathPiece::Polyline(w.map(|w| w.pts.clone()).unwrap_or_default()),
            });
        } else {
            let ci = c.ss.sketch.curve_index(&c.id).ok_or_else(|| DocError::Unknown(format!("path curve `{}`", c.id)))?;
            let segs = c.ss.sketch.exact_segs(ci).unwrap_or_else(|| c.ss.sketch.segs(ci));
            pieces.push(kernel::PathPiece::Segs(segs.iter().map(|s| kernel::PathSeg::of_sketch(&c.ss.plane, s)).collect()));
        }
    }
    // Pieces are put end to end from the end nearest `start`, whatever order they were picked
    // in (#16168); they must meet within the sketches' tolerance (projected curves are only
    // that close).
    let tol = 1e-3;
    let ends = |p: &kernel::PathPiece| {
        let pts = p.points();
        (pts.first().copied().unwrap_or_default(), pts.last().copied().unwrap_or_default())
    };
    let mut rest: Vec<kernel::PathPiece> = pieces;
    // The chain starts at a free end of the run, the one nearest `start`: a middle pick would
    // otherwise chain only onward from it and refuse the other side (#16168). A ring has no
    // free end; its first piece stays first.
    let near_start = |p: Vec3| if start.is_finite() { p.dist(start) } else { 0.0 };
    if rest.len() > 1 {
        let touches = |i: usize, p: Vec3| {
            rest.iter().enumerate().any(|(j, q)| {
                j != i && {
                    let (x, y) = ends(q);
                    x.dist(p) <= tol || y.dist(p) <= tol
                }
            })
        };
        let free = |i: usize| {
            let (a, b) = ends(&rest[i]);
            (!touches(i, a) || !touches(i, b)).then_some(i)
        };
        let end_dist = |i: usize| {
            let (a, b) = ends(&rest[i]);
            near_start(a).min(near_start(b))
        };
        if let Some(i) = (0..rest.len()).filter_map(free).min_by(|&x, &y| end_dist(x).total_cmp(&end_dist(y))) {
            rest.swap(0, i);
        }
    }
    let Some(first) = rest.first().cloned() else { return Ok(None) };
    rest.remove(0);
    let (a, b) = ends(&first);
    let flip = if start.is_finite() {
        b.dist(start) < a.dist(start)
    } else {
        rest.first().is_some_and(|n| {
            let (na, nb) = ends(n);
            na.dist(a).min(nb.dist(a)) < na.dist(b).min(nb.dist(b))
        })
    };
    let first = if flip { first.reversed() } else { first };
    let mut chain = vec![first];
    let mut last = ends(&chain[0]).1;
    while !rest.is_empty() {
        // The unused piece with the end nearest where the chain stands, reversed where the
        // chain runs against it.
        let mut best: Option<(usize, bool, f64)> = None;
        for (i, p) in rest.iter().enumerate() {
            let (a, b) = ends(p);
            let (da, db) = (a.dist(last), b.dist(last));
            if best.is_none_or(|(_, _, bd)| da.min(db) < bd) {
                best = Some((i, db < da, da.min(db)));
            }
        }
        let Some((i, reverse, d)) = best else { break };
        if d > tol {
            return Err(DocError::Invalid("the path curves are not connected end to end".into()));
        }
        let mut p = rest.swap_remove(i);
        if reverse {
            p = p.reversed();
        }
        last = ends(&p).1;
        chain.push(p);
    }
    Ok(Some(kernel::PointPath { pieces: chain }))
}

fn path_segments(ps: &SolvedSketch, ids: &[String], start: Vec3) -> Result<Vec<kernel::PathSeg>> {
    path_segments_at(&curves_in(ps, ids), start, None)
}

/// The path segments of a sweep's path on a body's edges (#2216): the chain the picked edges
/// make, each edge as the curve it is, reversed where the chain runs against it. An open chain
/// runs from the end nearest the profile, so the profile is swept from where it stands. A
/// closed chain begins at the profile instead: the edge the profile's plane crosses there
/// split in two, the chain rejoined from that point.
pub(crate) fn edge_path_segments(
    st: &ModelState,
    body: &crate::BodyName,
    edges: &[crate::Position],
    start: Vec3,
    profile: Option<&Plane>,
) -> Result<Vec<kernel::PathSeg>> {
    let Some(b) = st.body(&body.0) else { return Err(DocError::Unknown(format!("body `{body}`"))) };
    let points: Vec<Vec3> = crate::plain(edges);
    if points.is_empty() {
        return Err(DocError::Invalid("the path needs at least one edge".into()));
    }
    let chain = kernel::chain(&b.body, &points).map_err(|e| DocError::Invalid(format!("the path's edges: {e}")))?;
    // Each edge of the chain as its own path segments, in the chain's direction of travel.
    let mut groups: Vec<Vec<kernel::PathSeg>> = Vec::with_capacity(chain.len());
    for (edge, forward) in &chain {
        let mut s = kernel::edge_path(&b.body, *edge).map_err(|e| DocError::Invalid(format!("the path's edge: {e}")))?;
        if !*forward {
            s = s.into_iter().rev().map(|seg| seg.reversed()).collect();
        }
        groups.push(s);
    }
    if groups.iter().all(|g| g.is_empty()) {
        return Err(DocError::Invalid("the path's edges have no curve to follow".into()));
    }
    if kernel::chain_closes(&b.body, &chain) {
        // A closed chain is begun at the profile, so the profile is swept from where it stands.
        let segs = match profile {
            Some(profile) => from_profile_on_closed_edge_path(&b.body, &chain, groups, profile, start),
            None => groups.iter().flatten().copied().collect(),
        };
        return Ok(segs);
    }
    // The path runs from the end nearest the profile.
    let mut segs: Vec<kernel::PathSeg> = groups.iter().flatten().copied().collect();
    let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return Ok(segs) };
    if first.start().dist(start) > last.end().dist(start) {
        segs = segs.into_iter().rev().map(|seg| seg.reversed()).collect();
    }
    Ok(segs)
}

/// A closed chain of a body's edges begun where the profile's plane crosses it nearest the
/// profile: the edge it crosses there split in two, its second part first and its first part
/// last (or the chain begun at the joint there), so the path starts at the profile (#2216).
/// Unchanged when the chain's point nearest the profile is off that plane.
fn from_profile_on_closed_edge_path(
    body: &Body,
    chain: &[(usize, bool)],
    groups: Vec<Vec<kernel::PathSeg>>,
    profile: &Plane,
    start: Vec3,
) -> Vec<kernel::PathSeg> {
    let flat = |groups: &[Vec<kernel::PathSeg>]| groups.iter().flatten().copied().collect::<Vec<_>>();
    let record = body.record();
    let size: f64 = groups.iter().flatten().filter_map(kernel::PathSeg::length).sum();
    if !start.is_finite() || !(size > 0.0 && size.is_finite()) {
        return flat(&groups);
    }
    // The chain's point nearest the profile: the edge it is on, and the parameter there.
    let mut best: Option<(usize, f64, Vec3)> = None;
    for (k, (edge, _)) in chain.iter().enumerate() {
        let Some(e) = record.edges.get(*edge) else { continue };
        let Some((t, q)) = closest_on_curve(e.parameters(), e.interval, start) else { continue };
        if best.as_ref().is_none_or(|(_, _, bq)| q.dist(start) < bq.dist(start)) {
            best = Some((k, t, q));
        }
    }
    let Some((k, t, q)) = best else { return flat(&groups) };
    // The profile stands on the chain only where the chain meets the profile's plane.
    if profile.height(q).abs() > (size * 1e-6).max(1e-6) {
        return flat(&groups);
    }
    let (Some(g), Some(e)) = (groups.get(k), record.edges.get(chain[k].0)) else { return flat(&groups) };
    let (Some(first), Some(last)) = (g.first(), g.last()) else { return flat(&groups) };
    // The crossing at the edge's own joint: the chain is begun there, whole.
    if q.dist(first.start()) <= 1e-6 {
        return rotate(&groups, k);
    }
    if q.dist(last.end()) <= 1e-6 {
        return rotate(&groups, k + 1);
    }
    // The edge split in two at the crossing: the part the chain reaches first, and the part it
    // leaves last.
    let curve = e.parameters();
    let [t0, t1] = e.interval;
    let turned = |s: Vec<kernel::PathSeg>| s.into_iter().rev().map(|seg| seg.reversed()).collect::<Vec<_>>();
    let (before, after) = if chain[k].1 {
        match (kernel::curve_path(curve, [t0, t]), kernel::curve_path(curve, [t, t1])) {
            (Ok(a), Ok(b)) => (a, b),
            _ => return flat(&groups),
        }
    } else {
        match (kernel::curve_path(curve, [t, t1]), kernel::curve_path(curve, [t0, t])) {
            (Ok(a), Ok(b)) => (turned(a), turned(b)),
            _ => return flat(&groups),
        }
    };
    let mut out = Vec::with_capacity(groups.len() + 1);
    out.extend(after);
    out.extend(groups[k + 1..].iter().flatten().copied());
    out.extend(groups[..k].iter().flatten().copied());
    out.extend(before);
    out
}

/// The chain of a body's edges begun at its edge `k`, its edges in order.
fn rotate(groups: &[Vec<kernel::PathSeg>], k: usize) -> Vec<kernel::PathSeg> {
    let n = groups.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        out.extend(groups[(k + i) % n].iter().copied());
    }
    out
}

/// The point of the record curve `c` on `[t0, t1]` nearest `p`, and the parameter there: the
/// curve sampled and the nearest sample's piece narrowed.
fn closest_on_curve(c: &solidcore_record::brep::Curve, [t0, t1]: [f64; 2], p: Vec3) -> Option<(f64, Vec3)> {
    let eval = solidcraft_geom::brep::eval::CurveEval::new(c).ok()?;
    /// Samples along the curve to find the nearest piece.
    const SAMPLES: usize = 64;
    let step = (t1 - t0) / SAMPLES as f64;
    let mut best = (0usize, f64::INFINITY);
    for i in 0..=SAMPLES {
        let Some(q) = eval.point(t0 + step * i as f64) else { continue };
        let d = q.dist(p);
        if d < best.1 {
            best = (i, d);
        }
    }
    // Narrow the piece around the nearest sample (the distance is smallest near it).
    let (mut lo, mut hi) = (t0 + step * best.0.saturating_sub(1) as f64, t0 + step * (best.0 + 1).min(SAMPLES) as f64);
    for _ in 0..60 {
        let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
        let da = eval.point(a).map_or(f64::INFINITY, |q| q.dist(p));
        let db = eval.point(b).map_or(f64::INFINITY, |q| q.dist(p));
        if da < db {
            hi = b;
        } else {
            lo = a;
        }
    }
    let t = 0.5 * (lo + hi);
    Some((t, eval.point(t)?))
}

/// [`path_segments`] for a profile in `profile`: a closed path is followed from where the
/// profile's plane crosses it nearest `start`, the curve there split in two, when that is not
/// already where the chain starts (#3345). Swept from anywhere else, the profile would stand
/// off the path by the distance between the two.
pub(crate) fn path_segments_at(curves: &[PathCurve], start: Vec3, profile: Option<&Plane>) -> Result<Vec<kernel::PathSeg>> {
    if curves.is_empty() || curves.len() > 1000 {
        return Err(DocError::Invalid("the path needs 1…1000 curves".into()));
    }
    // A path through 3D sketch curves is followed as a 3D polyline.
    if let Some(path) = path_points(curves, start)? {
        return Ok(path.points().windows(2).map(|w| kernel::PathSeg::Line { a: w[0], b: w[1] }).collect());
    }
    // Each curve as its exact pieces (Béziers, conics) where it has them, on the plane of the
    // sketch it is in: the path may join curves of several sketches, whose planes differ
    // (#16168). A curve is turned round whole: its pieces stay one run.
    let mut curves: Vec<(Plane, Vec<solidcore_record::Seg2>)> = curves
        .iter()
        .map(|c| {
            let ci = c.ss.sketch.curve_index(&c.id).ok_or_else(|| DocError::Unknown(format!("path curve `{}`", c.id)))?;
            Ok((c.ss.plane, c.ss.sketch.exact_segs(ci).unwrap_or_else(|| c.ss.sketch.segs(ci))))
        })
        .collect::<Result<_>>()?;
    curves.retain(|(_, c)| !c.is_empty());
    let ends = |(plane, c): &(Plane, Vec<solidcore_record::Seg2>)| {
        c.first().zip(c.last()).map(|(f, l)| (plane.to_world(f.start()), plane.to_world(l.end())))
    };
    // Where two curves meet: one tolerance for telling which ends touch and for the joint.
    const JOINT: f64 = 1e-5;
    // Curves listed out of chain order are one chain all the same (#12199, #16168): when the
    // list is not already end to end, put each next curve where one of its ends meets the
    // chain's end.
    curves = chain_order(curves, JOINT, ends);
    // Orient the first curve: it runs toward the second where it meets it at one end only;
    // otherwise (one curve, or a second meeting it at both ends) from the end nearest `start`
    // (no start given, a pipe: as drawn).
    if let Some((a, b)) = curves.first().and_then(ends) {
        let touches = |p: Vec3| curves.get(1).and_then(ends).is_some_and(|(na, nb)| na.dist(p) <= JOINT || nb.dist(p) <= JOINT);
        let flip = match (touches(a), touches(b)) {
            (true, false) => true,
            (false, true) => false,
            _ => start.is_finite() && b.dist(start) < a.dist(start),
        };
        if flip && let Some(first) = curves.first_mut() {
            first.1 = std::mem::take(&mut first.1).into_iter().rev().map(|s| s.reversed()).collect();
        }
    }
    // Each next curve starts where the previous one ended; the curves are carried onto their
    // own sketch's plane as they join the chain, and the reversal is kept on the curve so the
    // closed-path split below reads the run as it is ordered here.
    let mut segs: Vec<kernel::PathSeg> = Vec::new();
    for (plane, curve) in &mut curves {
        let ends = |c: &[solidcore_record::Seg2]| c.first().zip(c.last()).map(|(f, l)| (plane.to_world(f.start()), plane.to_world(l.end())));
        if let Some(prev_end) = segs.last().map(kernel::PathSeg::end)
            && let Some((a, b)) = ends(curve)
            && b.dist(prev_end) < a.dist(prev_end)
        {
            *curve = std::mem::take(curve).into_iter().rev().map(|s| s.reversed()).collect();
        }
        segs.extend(curve.iter().map(|s| kernel::PathSeg::of_sketch(plane, s)));
    }
    if segs.windows(2).any(|pair| pair[1].start().dist(pair[0].end()) > JOINT) {
        return Err(DocError::Invalid("the path curves are not connected end to end".into()));
    }
    // The closed-path split works on the curves of one plane; a path across sketches that
    // closes is followed as listed.
    if let Some(profile) = profile
        && let Some((plane, _)) = curves.first()
        && curves.iter().all(|(p, _)| p == plane)
    {
        let flat: Vec<solidcore_record::Seg2> = curves.iter().flat_map(|(_, c)| c.iter().copied()).collect();
        let split = from_profile_on_closed_path(flat, plane, profile, start);
        return Ok(split.iter().map(|s| kernel::PathSeg::of_sketch(plane, s)).collect());
    }
    Ok(segs)
}

/// `curves` (each a run of segments) put into one chain, when they are not end to end as listed.
/// The chain starts at the first listed curve with an end that touches no other curve (the first
/// curve when they close into a ring) and goes on with the first listed unused curve that meets
/// its end. Curves that meet nothing keep their listed order after the chain, so a real gap is
/// still reported by the caller. `ends` gives each curve's two ends, in space (curves of
/// different sketches sit in different planes, #16168).
fn chain_order<T: Clone>(curves: Vec<T>, joint: f64, ends: impl Fn(&T) -> Option<(Vec3, Vec3)>) -> Vec<T> {
    let ends: Vec<Option<(Vec3, Vec3)>> = curves.iter().map(ends).collect();
    let meets = |p: Vec3, q: Vec3| p.dist(q) <= joint;
    // Already end to end as listed: nothing to do.
    let connected = ends.windows(2).all(|w| match (w[0], w[1]) {
        (Some((a, b)), Some((c, d))) => meets(a, c) || meets(a, d) || meets(b, c) || meets(b, d),
        _ => true,
    });
    if curves.len() < 3 || connected {
        return curves;
    }
    let n = curves.len();
    // Whether end `p` of curve `i` meets an end of any other curve.
    let shared = |i: usize, p: Vec3| (0..n).any(|j| j != i && ends[j].is_some_and(|(a, b)| meets(a, p) || meets(b, p)));
    let head = (0..n).find(|&i| ends[i].is_some_and(|(a, b)| !shared(i, a) || !shared(i, b))).or_else(|| (0..n).find(|&i| ends[i].is_some()));
    let Some(head) = head else { return curves };
    let mut used = vec![false; n];
    let mut order = vec![head];
    used[head] = true;
    // The head's open end is where the chain begins; it runs toward the other.
    let mut tail = match ends[head] {
        Some((a, b)) if !shared(head, a) => b,
        Some((a, _)) => a,
        None => return curves,
    };
    while let Some(next) = (0..n).find(|&j| !used[j] && ends[j].is_some_and(|(a, b)| meets(a, tail) || meets(b, tail))) {
        let Some((a, b)) = ends[next] else { break };
        tail = if meets(a, tail) { b } else { a };
        used[next] = true;
        order.push(next);
    }
    order.extend((0..n).filter(|&j| !used[j]));
    let mut slots: Vec<Option<T>> = curves.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots.get_mut(i).and_then(Option::take)).collect()
}

/// A closed chain `segs` (in the plane `path`) begun where `profile`'s plane crosses it nearest
/// `start`: the curve there split in two, its second part first and its first part last (or the
/// chain begun at the joint there). Unchanged when the chain is open, already starts on the
/// profile's plane, or its point nearest `start` is off that plane.
fn from_profile_on_closed_path(segs: Vec<solidcore_record::Seg2>, path: &Plane, profile: &Plane, start: Vec3) -> Vec<solidcore_record::Seg2> {
    let (Some(first), Some(last)) = (segs.first(), segs.last()) else { return segs };
    let size: f64 = segs.iter().map(solidcore_record::Seg2::length).sum();
    let on_plane = |p: Vec2| profile.height(path.to_world(p)).abs() <= (size * 1e-9).max(1e-9);
    if !start.is_finite() || !(size > 0.0 && size.is_finite()) || last.end().dist(first.start()) > 1e-5 || on_plane(first.start()) {
        return segs;
    }
    let local = path.to_local(start);
    let Some((k, t, q)) = segs
        .iter()
        .enumerate()
        .map(|(k, s)| {
            let t = s.closest_param(local);
            (k, t, path.to_world(s.point_at(t)))
        })
        .min_by(|a, b| a.2.dist(start).total_cmp(&b.2.dist(start)))
    else {
        return segs;
    };
    if profile.height(q).abs() > (size * 1e-6).max(1e-6) {
        return segs;
    }
    let Some(seg) = segs.get(k).copied() else { return segs };
    let n = segs.len();
    let from = |k: usize| segs.iter().cycle().skip(k % n).take(n).copied().collect::<Vec<_>>();
    let at = |p: Vec2| path.to_world(p).dist(q) <= 1e-6;
    if at(seg.start()) {
        return from(k);
    }
    if at(seg.end()) {
        return from(k + 1);
    }
    let (before, after) = seg.split_at(t);
    let mut out = Vec::with_capacity(n + 1);
    out.push(after);
    out.extend(from(k + 1).into_iter().take(n - 1));
    out.push(before);
    out
}

/// Where an extrude of distance `d` starts and ends along the sketch normal, before any start
/// offset: one side, the other, both (symmetric), or a second distance on the other side.
///
/// A second distance (`distance2`) goes the other way from the first side: against the normal,
/// or along it for `direction: negative` (#2700). Entered distances keep those sides whatever
/// their signs; a To Object below the sketch or a Through All that reaches down sends the first
/// side down and the second back along the normal.
pub(crate) fn extent_sides(vals: &BTreeMap<String, Value>, extent: &crate::Extent, d: f64, direction: crate::Direction) -> Result<(f64, f64)> {
    Ok(match (&extent.distance2, direction) {
        (Some(d2), _) => {
            let negative = direction == crate::Direction::Negative;
            let first = if negative { -d } else { d };
            let d2 = val(vals, d2, Kind::Length)?;
            // An entered distance keeps its side whatever its sign: Direction fixes the sides,
            // and a sign only turns its own side over. A reach computed from the model (To
            // Object, Through All) has the sign of where the object is, so the other side
            // follows it.
            let second = if extent.second_follows_first || extent.to.is_some() || extent.through_all {
                if first < 0.0 { d2 } else { -d2 }
            } else if negative {
                d2
            } else {
                -d2
            };
            (second, first)
        }
        (None, crate::Direction::Positive) => (0.0, d),
        (None, crate::Direction::Negative) => (-d, 0.0),
        (None, crate::Direction::Symmetric) => {
            // Through All is a reach computed from target bodies, not an entered total.
            let half = if extent.through_all { d } else { extent.convention.symmetric_half_distance(d) };
            (-half, half)
        }
    })
}

/// Operation and targets of a feature that makes tools.
fn feature_op(f: &Feature) -> (Operation, Vec<String>) {
    match &f.kind {
        FeatureKind::Extrude { operation, targets, .. } | FeatureKind::Revolve { operation, targets, .. } => (*operation, crate::plain(targets)),
        FeatureKind::Box { operation, .. }
        | FeatureKind::Cylinder { operation, .. }
        | FeatureKind::Sphere { operation, .. }
        | FeatureKind::Torus { operation, .. }
        | FeatureKind::Cone { operation, .. } => (*operation, Vec::new()),
        FeatureKind::Hole { .. } => (Operation::Cut, Vec::new()),
        FeatureKind::Pipe { operation, targets, .. } => (*operation, crate::plain(targets)),
        FeatureKind::Loft { operation, targets, .. } | FeatureKind::Sweep { operation, targets, .. } => (*operation, crate::plain(targets)),
        FeatureKind::Coil { operation, targets, .. } => (*operation, crate::plain(targets)),
        FeatureKind::CellGrid { targets, mode, .. } => (
            match mode {
                crate::CellGridMode::Cut => Operation::Cut,
                crate::CellGridMode::Raised => Operation::Join,
            },
            crate::plain(targets),
        ),
        FeatureKind::Rib { .. } => (Operation::Join, Vec::new()),
        _ => (Operation::NewBody, Vec::new()),
    }
}

type Mat = [[f64; 4]; 4];

fn translation(t: Vec3) -> Mat {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [t.x, t.y, t.z, 1.0]]
}

/// Rotation by `ang` about the line through `o` with direction `a` (column-major).
fn rotation(o: Vec3, a: Vec3, ang: f64) -> Mat {
    let a = a.normalized().unwrap_or(Vec3::Z);
    let (c, s) = (ang.cos(), ang.sin());
    let t = 1.0 - c;
    let r = [
        [t * a.x * a.x + c, t * a.x * a.y + s * a.z, t * a.x * a.z - s * a.y],
        [t * a.x * a.y - s * a.z, t * a.y * a.y + c, t * a.y * a.z + s * a.x],
        [t * a.x * a.z + s * a.y, t * a.y * a.z - s * a.x, t * a.z * a.z + c],
    ];
    // p' = R (p - o) + o
    let ro = Vec3::new(
        r[0][0] * o.x + r[1][0] * o.y + r[2][0] * o.z,
        r[0][1] * o.x + r[1][1] * o.y + r[2][1] * o.z,
        r[0][2] * o.x + r[1][2] * o.y + r[2][2] * o.z,
    );
    let tr = o - ro;
    [[r[0][0], r[0][1], r[0][2], 0.0], [r[1][0], r[1][1], r[1][2], 0.0], [r[2][0], r[2][1], r[2][2], 0.0], [tr.x, tr.y, tr.z, 1.0]]
}

/// A copy of a sheet metal body is sheet metal too: its sheet goes with it, moved, or mirrored
/// into the opposite hand ([`crate::SurfaceSheet::copied`]), so the copy has its own flat
/// pattern, DXF and bend table. A copy by any other transform stays a plain solid.
fn copy_sheet(st: &mut ModelState, from: &str, to: &str, m: &crate::Mat) {
    let Some(sheet) = st.surface_sheets.iter().find(|s| s.body == from).and_then(|s| s.copied(to, m)) else { return };
    st.surface_sheets.retain(|s| s.body != to);
    st.surface_sheets.push(sheet);
}

/// Reflection in a plane (column-major).
fn mirror_matrix(pl: &Plane) -> Mat {
    let n = pl.normal();
    let d = pl.origin.dot(n);
    let m = |i: usize, j: usize| {
        let (ni, nj) = ([n.x, n.y, n.z][i], [n.x, n.y, n.z][j]);
        (if i == j { 1.0 } else { 0.0 }) - 2.0 * ni * nj
    };
    // p' = p - 2 (p·n - d) n
    [
        [m(0, 0), m(0, 1), m(0, 2), 0.0],
        [m(1, 0), m(1, 1), m(1, 2), 0.0],
        [m(2, 0), m(2, 1), m(2, 2), 0.0],
        [2.0 * d * n.x, 2.0 * d * n.y, 2.0 * d * n.z, 1.0],
    ]
}

/// The most copies one pattern makes (the original included), and so one mirror or pattern step
/// of nested ones; perforated plates and perfboard grids need a few thousand (#3042).
const MAX_INSTANCES: usize = 10_000;

/// The copies of body `n` (source `s` of `count`) a body pattern makes, one per matrix of
/// `mats` past the first. Picked where a placement (`path`) shows it, the body is copied where
/// that placement shows it, and its copies go into the component that placement shows, as a
/// mirror's do. A body of another component, picked by its own name, is copied as the pattern
/// moves it in the world and kept beside its source, in its component (#4763).
#[allow(clippy::too_many_arguments)]
fn pattern_body(doc: &Document, f: &Feature, st: &mut ModelState, mats: &[Mat], count: usize, s: usize, n: &str, path: Option<&[u64]>) -> Result<()> {
    let mb = st.body(n).cloned().ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
    let here = doc.component_transform(f.component);
    let singular = || DocError::Invalid(format!("the component holding `{n}` has a singular placement"));
    // How a pattern matrix (in this feature's component) moves the body's own geometry, and
    // the component its copies go into when it is not this feature's.
    let (component, frames) = match mirrored_placement(doc, f.component, &mb, path)? {
        Some((component, frame, source)) => {
            let inverse = |m: &Mat| crate::mat_inverse(m).ok_or_else(singular);
            (Some(component), Some((inverse(&frame)?, source)))
        }
        None if mb.component != f.component => {
            let frame = doc.component_transform(mb.component);
            let inverse = |m: &Mat| crate::mat_inverse(m).ok_or_else(singular);
            (Some(mb.component), Some((crate::mat_mul(&inverse(&frame)?, &here), crate::mat_mul(&inverse(&here)?, &frame))))
        }
        None => (None, None),
    };
    // Named against an index of the names in use: searching every body for every name tried
    // took time cubic in the copies.
    let mut names = BodyNames::new(st);
    let mut made = Vec::with_capacity(mats.len());
    for (k, m) in mats.iter().enumerate() {
        let local = match &frames {
            Some((into, from)) => crate::mat_mul(into, &crate::mat_mul(m, from)),
            None => *m,
        };
        let copy = moved_copy(&mb, kernel::transform_matrix(&mb.body, local)?, k + 1);
        let slot = pattern_name_slot(f.body_names.len(), count, mats.len(), s, k, f.pattern_names_in_order);
        let name = pattern_copy_name(&mut names, f, slot, &mb.name);
        made.push((name, copy, local));
    }
    // Only once every copy of this body is made: a body that fails leaves none behind.
    for (name, copy, local) in made {
        copy_sheet(st, &mb.name, &name, &local);
        let mut body = ModelBody::traced_new(name, copy, f.id);
        // A copy is its source moved: it is drawn from the source's triangles and display plans,
        // moved, rather than meshed and planned again for every copy.
        body.mesh_placement = placed_mesh::PlacedMesh::new(&mb, &local).map(Arc::new);
        if let Some(c) = component.filter(|c| *c != f.component) {
            // Kept in that component as a moved body is (and through later features).
            body.component = c;
            body.transferred = true;
        }
        st.bodies.push(body);
    }
    Ok(())
}

/// The warning of a body pattern that left out the bodies `skipped`, the first for `why`.
fn pattern_skipped_note(skipped: &[String], why: &DocError) -> String {
    let shown: Vec<&str> = skipped.iter().take(3).map(String::as_str).collect();
    let more = if skipped.len() > 3 { format!(" (+{} more)", skipped.len() - 3) } else { String::new() };
    format!("{} of the bodies could not be copied and were left out: {}{more} ({why})", skipped.len(), shown.join(", "))
}

/// Where a mirrored body was picked and where its copy goes, for a mirror in component `own`: the
/// component the copy joins and that component's frame, and the frame the body's own geometry is
/// in where it was picked (its placement `path`, with its offset), both in `own`'s frame. `None` in
/// older designs, which saved no placement and mirror the body's own geometry.
fn mirrored_placement(doc: &Document, own: u64, mb: &ModelBody, path: Option<&[u64]>) -> Result<Option<(u64, Mat, Mat)>> {
    let Some(path) = path else { return Ok(None) };
    let lost = || DocError::Invalid(format!("the placement `{}` was mirrored at no longer exists", mb.name));
    // A stale or edited path that leads to another component no longer shows the body.
    let (at, _) = doc.path_placement(path).ok_or_else(lost)?;
    if !placement_shows(doc, mb, at) {
        return Err(lost());
    }
    let singular = || DocError::Invalid(format!("the placement `{}` was mirrored at is singular", mb.name));
    let (component, frame, picked) = mirror_copy_frame(doc, own, path).ok_or_else(singular)?;
    let source = doc.model_body_offset(mb).map(|offset| crate::mat_mul(&picked, offset)).unwrap_or(picked);
    Ok(Some((component, frame, source)))
}

/// Whether a placement of component `at` shows body `mb`: `at` is the body's component. That is
/// the evaluated body's own where a transfer recorded it, else the component its name is recorded
/// in (an import's bodies) or its feature's. Inserting the same file again reuses the body names
/// and points them at the new insert's components: when the recorded component lies outside the
/// components the body's own feature made, the name is another body's, and the placement only has
/// to lie inside the components the body's feature made.
fn placement_shows(doc: &Document, mb: &ModelBody, at: u64) -> bool {
    let owner = doc.model_body_component(mb);
    if owner == at {
        return true;
    }
    if mb.transferred || mb.source_name.is_some() {
        return false;
    }
    let made_in = doc.feature(mb.feature).map(|f| f.component).unwrap_or(0);
    !component_within(doc, owner, made_in) && component_within(doc, at, made_in)
}

/// Whether component `c` is `ancestor` or lies inside it (every component lies in the root).
fn component_within(doc: &Document, mut c: u64, ancestor: u64) -> bool {
    for _ in 0..=doc.components.len() {
        if c == ancestor || ancestor == 0 {
            return true;
        }
        match doc.components.iter().find(|x| x.id == c) {
            Some(x) if c != 0 => c = x.parent,
            _ => return false,
        }
    }
    false
}

/// For a body mirrored at placement `path` (which, not the body's name, says where it is: a later
/// insert can reuse the name) by a mirror in component `own`: the component its copy joins, that
/// component's frame, and the placement's frame, both in `own`'s frame ([`frame_in_own`]). The copy
/// joins the body's component, or, when that component is placed more than once (a copy there
/// would show at every placement), the innermost component around it that is placed once.
fn mirror_copy_frame(doc: &Document, own: u64, path: &[u64]) -> Option<(u64, Mat, Mat)> {
    let placed_once = |id: &u64| {
        let component = doc.occurrences.iter().find(|o| o.id == *id).map(|o| o.component);
        doc.occurrences.iter().filter(|o| Some(o.component) == component).count() == 1
    };
    let once = path.iter().take_while(|id| placed_once(id)).count();
    let (component, _) = doc.path_placement(path.get(..once)?)?;
    Some((component, frame_in_own(doc, own, path.get(..once)?)?, frame_in_own(doc, own, path)?))
}

/// The frame placement `path` leads to, in the frame of `component` (its first placement, as
/// [`Document::component_transform`] places it): the occurrences of `path` past those the two
/// placements share, after the inverse of `component`'s own past them. What they share cancels
/// exactly, so a joint that moves both together changes nothing, not even by rounding (#14271).
/// Through the world when `component`'s placement cannot be walked. `None` when `path` leads
/// nowhere or the frame is singular.
fn frame_in_own(doc: &Document, component: u64, path: &[u64]) -> Option<Mat> {
    let Some(own) = first_placement_path(doc, component) else {
        let (_, world) = doc.path_placement(path)?;
        return Some(crate::mat_mul(&crate::mat_inverse(&doc.component_transform(component))?, &world));
    };
    let shared = own.iter().zip(path).take_while(|(a, b)| a == b).count();
    let (at, _) = doc.path_placement(path.get(..shared)?)?;
    let down = occurrences_from(doc, at, path.get(shared..)?)?;
    match own.get(shared..) {
        Some([]) => Some(down),
        Some(up) => Some(crate::mat_mul(&crate::mat_inverse(&occurrences_from(doc, at, up)?)?, &down)),
        None => None,
    }
}

/// Component `c`'s frame in the frame of component `own` ([`frame_in_own`]); `None` when the two
/// are one frame, so a copy made in `own`'s frame needs no change there (#19451).
fn component_in(doc: &Document, own: u64, c: u64) -> Option<Mat> {
    if c == own {
        return None;
    }
    let m = match first_placement_path(doc, c) {
        Some(path) => frame_in_own(doc, own, &path)?,
        None => crate::mat_mul(&crate::mat_inverse(&doc.component_transform(own))?, &doc.component_transform(c)),
    };
    (m != crate::IDENTITY).then_some(m)
}

/// The frame body `b`'s geometry is in, in component `own`'s frame: [`component_in`] with the
/// body's offset. `None` when it is `own`'s frame.
fn body_in(doc: &Document, own: u64, b: &ModelBody) -> Option<Mat> {
    let frame = component_in(doc, own, doc.model_body_component(b));
    match doc.model_body_offset(b) {
        Some(offset) => Some(crate::mat_mul(&frame.unwrap_or(crate::IDENTITY), offset)).filter(|m| *m != crate::IDENTITY),
        None => frame,
    }
}

/// Copy transform `m` as it acts in the frame placed at `at` in the frame `m` is in.
fn moved_in(m: &Mat, at: &Mat) -> Result<Mat> {
    let back = crate::mat_inverse(at).ok_or_else(|| DocError::Invalid("a placement the copies are made through is singular".into()))?;
    Ok(crate::mat_mul(&crate::mat_mul(&back, m), at))
}

/// The occurrences of `component`'s first placement from the root, as
/// [`Document::component_transform`] walks them. `None` when a component on the way has no
/// occurrence or the walk is deeper than any placement.
fn first_placement_path(doc: &Document, component: u64) -> Option<Vec<u64>> {
    let mut ids = Vec::new();
    let mut c = component;
    while c != 0 {
        if ids.len() > crate::MAX_PLACEMENT_DEPTH {
            return None;
        }
        let o = doc.occurrence_of(c)?;
        ids.push(o.id);
        c = o.parent;
    }
    ids.reverse();
    Some(ids)
}

/// The transform of the occurrences `ids` in turn from component `from`: the frame they lead to,
/// in `from`'s frame. `None` when they no longer lead anywhere.
fn occurrences_from(doc: &Document, from: u64, ids: &[u64]) -> Option<Mat> {
    let (mut m, mut c) = (crate::IDENTITY, from);
    for id in ids {
        let o = doc.occurrences.iter().find(|o| o.id == *id && o.parent == c)?;
        m = crate::mat_mul(&m, &o.transform);
        c = o.component;
    }
    Some(m)
}

/// The placements (column-major matrices) of a pattern's copies, the original excluded, read
/// with the values the features are built with, so a bare number in an expression is in the
/// design's unit (#8523).
pub fn pattern_matrices(doc: &Document, st: &ModelState, p: &crate::PatternKind) -> Result<Vec<[[f64; 4]; 4]>> {
    let vals = doc.evaluation_values();
    pattern_transforms(doc, &vals, st, 0, p)
}

/// A pattern count: a whole number of 1 to [`MAX_INSTANCES`].
fn pattern_count(vals: &BTreeMap<String, Value>, e: &str) -> Result<usize> {
    let n = expr::eval_count(e, vals)?;
    if !(1.0..=MAX_INSTANCES as f64).contains(&n) {
        return Err(DocError::Invalid(format!("pattern count must be 1…{MAX_INSTANCES}")));
    }
    Ok(n as usize)
}

/// One direction of a rectangular pattern, worked out from its distance type (#8523).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternRow {
    /// How many, the original included.
    pub count: usize,
    /// The step between neighbours.
    pub step: f64,
    /// Copy at the end: the length the row spans, its last copy at the extent's end and the gap
    /// before it shorter than `step` (centred: half of it each way, a copy at either end).
    pub end: Option<f64>,
}

impl PatternRow {
    /// Each one's offset along the direction in slot order, the original's (zero) first, placed
    /// by the alignment and Flip as the copies always were. With Copy at the end, the last slot
    /// moves to where the whole row spans exactly the extent; centred, the outermost slot on
    /// each side moves to half the extent that way.
    pub fn offsets(&self, alignment: solidcraft_sketch::PatternAlignment, reversed: bool) -> Vec<f64> {
        let mut out: Vec<f64> = (0..self.count).map(|i| self.step * alignment.multiplier(i, reversed)).collect();
        let last = self.count.saturating_sub(1);
        if let Some(end) = self.end
            && alignment == solidcraft_sketch::PatternAlignment::Center
        {
            let outer = (0..self.count).map(|i| alignment.multiplier(i, reversed).abs()).fold(0.0_f64, f64::max);
            for (i, slot) in out.iter_mut().enumerate() {
                let m = alignment.multiplier(i, reversed);
                if outer > 0.0 && m.abs() == outer {
                    *slot = end / 2.0 * m.signum();
                }
            }
        } else if let Some(end) = self.end
            && last > 0
        {
            let others = out.iter().take(last);
            let lo = others.clone().copied().fold(0.0_f64, f64::min);
            let hi = others.copied().fold(0.0_f64, f64::max);
            let ahead = alignment.multiplier(last, reversed) >= 0.0;
            if let Some(slot) = out.get_mut(last) {
                *slot = if ahead { lo + end } else { hi - end };
            }
        }
        out
    }
}

/// One rectangular pattern direction from its distance type and the expressions it uses: the
/// count, the spacing and the extent (one it doesn't use may be anything, or left out). A
/// centred direction set by spacing and extent reaches half the extent each way from the
/// original, with as many copies on each side.
pub fn pattern_row(
    vals: &BTreeMap<String, Value>,
    distance: crate::PatternDistance,
    count: Option<&str>,
    spacing: Option<&str>,
    extent: Option<&str>,
    copy_at_end: bool,
    alignment: solidcraft_sketch::PatternAlignment,
) -> Result<PatternRow> {
    use crate::PatternDistance as D;
    let missing = |what: &str| DocError::Invalid(format!("the pattern's {what} is missing"));
    let positive = |e: Option<&str>, what: &str| -> Result<f64> {
        let v = val(vals, e.ok_or_else(|| missing(what))?, Kind::Length)?;
        if !v.is_finite() || v <= 0.0 || v >= 1e9 {
            return Err(DocError::Invalid(format!("the pattern's {what} must be greater than zero")));
        }
        Ok(v)
    };
    match distance {
        D::CountSpacing => {
            let n = pattern_count(vals, count.ok_or_else(|| missing("count"))?)?;
            let step = val(vals, spacing.ok_or_else(|| missing("spacing"))?, Kind::Length)?;
            Ok(PatternRow { count: n, step, end: None })
        }
        D::CountExtent => {
            let n = pattern_count(vals, count.ok_or_else(|| missing("count"))?)?;
            let extent = positive(extent, "extent")?;
            let step = if n > 1 { extent / (n - 1) as f64 } else { 0.0 };
            Ok(PatternRow { count: n, step, end: None })
        }
        D::SpacingExtent => {
            let step = positive(spacing, "spacing")?;
            let extent = positive(extent, "extent")?;
            let centred = alignment == solidcraft_sketch::PatternAlignment::Center;
            // How far the copies reach on each side they go to.
            let reach = if centred { extent / 2.0 } else { extent };
            // Float noise such as 0.1 + 0.2 must not lose the copy at the extent's end.
            let tol = 1e-9 * extent.max(1.0);
            if step > reach + tol {
                let what = if centred { "half its extent" } else { "its extent" };
                return Err(DocError::Invalid(format!("the pattern's spacing ({step} mm) is longer than {what} ({extent} mm)")));
            }
            let sides = if centred { 2 } else { 1 };
            let fit = ((reach + tol) / step).floor();
            if !fit.is_finite() || fit * sides as f64 >= MAX_INSTANCES as f64 {
                return Err(DocError::Invalid(format!("pattern count must be 1…{MAX_INSTANCES}")));
            }
            let gaps = fit as usize * sides;
            let short = reach - fit * step > tol;
            if copy_at_end && short {
                return Ok(PatternRow { count: gaps + 1 + sides, step, end: Some(extent) });
            }
            Ok(PatternRow { count: gaps + 1, step, end: None })
        }
    }
}

/// Keep each Pattern of components' occurrences in step with its count and spacing (#12116):
/// add or remove copies when the count changes, update transforms when the source or spacing
/// moves, and retain copies of a suppressed or rolled-back Pattern for later reactivation.
/// Surviving copies keep their occurrence ids.
pub fn reconcile_component_patterns(doc: &mut Document, st: &ModelState) -> Result<()> {
    let marker = doc.marker.unwrap_or(doc.features.len());
    // The values the features are built with, as `pattern_matrices` reads them when the copies
    // are first placed: a bare number is in the design's unit (#8523).
    let vals = doc.evaluation_values();
    // Snapshot each Pattern of components before mutating the document. The placement tree
    // excludes the stored copies of suppressed and rolled-back patterns.
    let plans: Vec<(u64, bool, Vec<u64>, crate::PatternKind, u64)> = doc
        .features
        .iter()
        .enumerate()
        .filter_map(|(i, f)| match &f.kind {
            FeatureKind::Pattern { pattern, components, .. } if !components.is_empty() => {
                Some((f.id, f.suppressed || i >= marker, components.clone(), pattern.clone(), f.component))
            }
            _ => None,
        })
        .collect();
    for (feature_id, inactive, components, pattern, feature_component) in plans {
        if inactive {
            continue;
        }
        let mats = pattern_transforms(doc, &vals, st, feature_component, &pattern)?;
        let mut made = Vec::with_capacity(components.len().saturating_mul(mats.len()));
        let previous = match &doc.feature(feature_id).map(|f| &f.kind) {
            Some(FeatureKind::Pattern { occurrences, .. }) => occurrences.clone(),
            _ => Vec::new(),
        };
        // Occurrences are stored per component, then per matrix. Shrink/grow each component's
        // block on its own so survivors keep their ids when the count changes.
        let old_per = match components.len() {
            0 => 0,
            n => previous.len() / n,
        };
        let mut leftover = std::collections::BTreeSet::new();
        for (ci, component) in components.into_iter().enumerate() {
            let src = doc.occurrence_of(component).cloned().ok_or_else(|| DocError::Unknown(format!("component {component}")))?;
            // The first placement is the pattern's seed; keep it by id so later adds of this
            // reconcile do not change which occurrence we read as the source.
            let seed = src.id;
            let src = doc.occurrences.iter().find(|o| o.id == seed).cloned().unwrap_or(src);
            let start = ci.saturating_mul(old_per);
            let block = previous.get(start..start.saturating_add(old_per)).unwrap_or(&[]);
            let mut old = block.iter().copied();
            for m in &mats {
                let transform = crate::mat_mul(m, &src.transform);
                match old.next() {
                    Some(id) if doc.occurrences.iter().any(|o| o.id == id && o.component == component) => {
                        if let Some(o) = doc.occurrences.iter_mut().find(|o| o.id == id) {
                            o.parent = src.parent;
                            o.transform = transform;
                        }
                        made.push(id);
                    }
                    Some(id) => {
                        leftover.insert(id);
                        made.push(doc.add_occurrence(component, src.parent, transform)?);
                    }
                    None => {
                        made.push(doc.add_occurrence(component, src.parent, transform)?);
                    }
                }
            }
            leftover.extend(old);
        }
        if !leftover.is_empty() {
            doc.remove_occurrences(&leftover);
        }
        if let Some(FeatureKind::Pattern { occurrences, .. }) = doc.feature_mut(feature_id).map(|f| &mut f.kind) {
            *occurrences = made;
        }
    }
    Ok(())
}

fn pattern_transforms(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, component: u64, p: &crate::PatternKind) -> Result<Vec<Mat>> {
    let count = |e: &str| pattern_count(vals, e);
    let mut out = Vec::new();
    match p {
        crate::PatternKind::Rectangular {
            dir1,
            count1,
            spacing1,
            dir2,
            count2,
            spacing2,
            alignment,
            alignment2,
            reversed,
            reversed2,
            distance1,
            distance2,
            extent1,
            extent2,
            copy_at_end1,
            copy_at_end2,
        } => {
            let row1 = pattern_row(vals, *distance1, Some(count1), Some(spacing1), extent1.as_deref(), *copy_at_end1, *alignment)?;
            let d1 = dir1.normalized().ok_or_else(|| DocError::Invalid("pattern direction".into()))?;
            // A second direction without the values its distance type uses is left out, as one
            // without a count or a spacing always was.
            let complete = (!distance2.uses_count() || count2.is_some())
                && (!distance2.uses_spacing() || spacing2.is_some())
                && (!distance2.uses_extent() || extent2.is_some());
            let (row2, d2) = match dir2 {
                Some(d) if complete => (
                    pattern_row(vals, *distance2, count2.as_deref(), spacing2.as_deref(), extent2.as_deref(), *copy_at_end2, *alignment2)?,
                    d.normalized().ok_or_else(|| DocError::Invalid("pattern direction".into()))?,
                ),
                _ => (PatternRow { count: 1, step: 0.0, end: None }, Vec3::Y),
            };
            if row1.count.saturating_mul(row2.count) > MAX_INSTANCES {
                return Err(DocError::Invalid("too many pattern instances".into()));
            }
            let (along1, along2) = (row1.offsets(*alignment, *reversed), row2.offsets(*alignment2, *reversed2));
            for (i, a) in along1.iter().enumerate() {
                for (j, b) in along2.iter().enumerate() {
                    if i == 0 && j == 0 {
                        continue;
                    }
                    out.push(translation(d1 * *a + d2 * *b));
                }
            }
        }
        crate::PatternKind::Path { path_sketch, path, count: c, spacing, extent, flip, orient, start, start_fraction, follow_frame } => {
            let n = count(c)?;
            let d = val(vals, spacing, Kind::Length)?;
            let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("path sketch {path_sketch}")))?;
            let mapped = path_in_frame(doc, ps, component)?;
            let ps = mapped.as_ref();
            // Free-form curves measured along their polylines.
            let mut segs = kernel::PathSeg::flattened(&path_segments(ps, path, Vec3::new(f64::NAN, f64::NAN, f64::NAN))?);
            if *flip {
                segs = segs.iter().rev().map(reverse_seg).collect();
            }
            let total: f64 = segs.iter().map(seg_len).sum();
            let step = if *extent { if n > 1 { d / (n - 1) as f64 } else { 0.0 } } else { d };
            // Where the original sits along the path: the start point, a distance or a fraction
            // of the path's length (so it keeps its place as the path changes).
            let from = match (start_fraction, start) {
                (Some(f), _) => {
                    let f = val(vals, f, Kind::Unitless)?;
                    if !(0.0..=1.0).contains(&f) {
                        return Err(DocError::Invalid("the start point must be 0…1 of the path's length".into()));
                    }
                    Some(f * total)
                }
                (None, Some(s)) => {
                    let s = val(vals, s, Kind::Length)?;
                    if !s.is_finite() || s < -1e-6 || s > total * (1.0 + 1e-5) + 1e-6 {
                        return Err(DocError::Invalid(format!("the start point must lie on the path (0…{total:.3} mm)")));
                    }
                    Some(s.clamp(0.0, total))
                }
                (None, None) => None,
            };
            let last = from.unwrap_or(0.0) + step * n.saturating_sub(1) as f64;
            // Without a start point, as before: the copies' span must fit the path.
            let past_end = match from {
                None => (step * n.saturating_sub(1) as f64).abs() > total * (1.0 + 1e-5) + 1e-6,
                Some(_) => !last.is_finite() || last > total * (1.0 + 1e-5) + 1e-6,
            };
            if past_end {
                return Err(DocError::Invalid(format!("the instances run past the end of the path ({total:.3} mm long)")));
            }
            if from.is_some() && last < -1e-6 {
                return Err(DocError::Invalid(format!("the instances run past the start of the path ({total:.3} mm long)")));
            }
            let from = from.unwrap_or(0.0);
            let normal = ps.plane.normal();
            let (p0, r0) = path_station(&segs, from, normal);
            for i in 1..n {
                let at = from + step * i as f64;
                let (pk, rk) = path_station(&segs, at, normal);
                out.push(match (*orient, *follow_frame) {
                    (false, _) => translation(pk - p0),
                    (true, true) => frame_to_frame(p0, &r0, pk, &rk),
                    // A pattern saved before the frame: the signed turn of the tangent about the
                    // sketch's normal only.
                    (true, false) => {
                        let (t0, tk) = (path_tangent(&segs, from), path_tangent(&segs, at));
                        let mut m = rotation(p0, normal, t0.cross(tk).dot(normal).atan2(t0.dot(tk)));
                        let dv = pk - p0;
                        m[3][0] += dv.x;
                        m[3][1] += dv.y;
                        m[3][2] += dv.z;
                        m
                    }
                });
            }
        }
        crate::PatternKind::Circular { origin, axis, count: c, angle, alignment, reversed, axis_ref, axis_feature } => {
            // On an edge or round face: about it as it is now; on a construction axis: where it is;
            // else through its origin, whose expressions follow the parameters (#10852).
            let (origin, axis) = match (axis_ref, axis_feature) {
                (Some(g), _) => crate::construct::model_axis_in(Some(doc), &*construction_state(doc, st, component)?, g, component)?,
                (None, Some(name)) => construction_axis_in(Some(doc), st, name, component)?,
                (None, None) => {
                    let [x, y, z] = origin;
                    let o = Vec3::new(val(vals, x, Kind::Length)?, val(vals, y, Kind::Length)?, val(vals, z, Kind::Length)?);
                    if ![o.x, o.y, o.z].iter().all(|c| c.is_finite() && c.abs() < 1e9) {
                        return Err(DocError::Invalid("the pattern's axis origin is out of range".into()));
                    }
                    (o, axis.0)
                }
            };
            let n = count(c)?;
            let total = val(vals, angle, Kind::Angle)?;
            let full = (total.abs() - std::f64::consts::TAU).abs() < 1e-9;
            let step = if full {
                total / n as f64
            } else if n > 1 {
                total / (n - 1) as f64
            } else {
                0.0
            };
            for i in 1..n {
                out.push(rotation(origin, axis, step * alignment.multiplier(i, *reversed)));
            }
        }
    }
    Ok(out)
}

fn seg_len(s: &kernel::PathSeg) -> f64 {
    match *s {
        kernel::PathSeg::Line { a, b } => (b - a).len(),
        kernel::PathSeg::Arc { a, center, angle, .. } => (a - center).len() * angle.abs(),
        // Patterns flatten their paths; a curve left is measured along its chord.
        kernel::PathSeg::Curve { .. } | kernel::PathSeg::Bezier(_) => s.start().dist(s.end()),
    }
}

/// Rotate `v` about the unit axis `k` by `ang` (Rodrigues).
fn rotate_vec(v: Vec3, k: Vec3, ang: f64) -> Vec3 {
    v * ang.cos() + k.cross(v) * ang.sin() + k * (k.dot(v) * (1.0 - ang.cos()))
}

/// Point and unit tangent `t` mm along a segment.
fn seg_at(s: &kernel::PathSeg, t: f64) -> (Vec3, Vec3) {
    match *s {
        kernel::PathSeg::Line { a, b } => {
            let d = (b - a).normalized().unwrap_or(Vec3::X);
            (a + d * t, d)
        }
        kernel::PathSeg::Arc { a, center, axis, angle } => {
            let k = axis.normalized().unwrap_or(Vec3::Z);
            let r = (a - center).len().max(1e-12);
            // Sweeps turn right-handed about `axis` (angles are positive).
            let s = if angle < 0.0 { -1.0 } else { 1.0 };
            let v = rotate_vec(a - center, k, s * t / r);
            (center + v, k.cross(v).normalized().unwrap_or(Vec3::X) * s)
        }
        kernel::PathSeg::Curve { .. } | kernel::PathSeg::Bezier(_) => {
            let (a, b) = (s.start(), s.end());
            let d = (b - a).normalized().unwrap_or(Vec3::X);
            (a + d * t, d)
        }
    }
}

fn reverse_seg(s: &kernel::PathSeg) -> kernel::PathSeg {
    s.reversed()
}

/// Where a chain is `d` mm along (clamped to its ends), and how it has turned to get there from
/// its start: a rotation-minimising frame, the turn of each arc about its own axis and, at each
/// joint, the least turn from one segment's direction to the next one's. The frame turns with
/// the tangent and never rolls about it, as a swept profile does, so on a 3D path a copy keeps
/// the relation to the path that the original has; on a planar path it is the turn about the
/// sketch's normal. `normal` (the sketch's) is the axis of a half turn where the path doubles
/// back on itself. A segment with no length (a point repeated where a spline is sampled) has no
/// direction: it turns nothing, and the next segment turns from the last one that had one.
fn path_station(segs: &[kernel::PathSeg], d: f64, normal: Vec3) -> (Vec3, crate::Mat) {
    let mut r = crate::IDENTITY;
    let mut left = d.max(0.0);
    let mut came_along: Option<Vec3> = None;
    for (i, s) in segs.iter().enumerate() {
        let l = seg_len(s);
        let last = left <= l + 1e-9 || i + 1 == segs.len();
        if l <= solidcraft_geom::EPS {
            if last {
                return (s.end(), r);
            }
            left -= l;
            continue;
        }
        let (_, t_in) = seg_at(s, 0.0);
        if let Some(t) = came_along {
            r = crate::mat_mul(&least_turn(t, t_in, normal), &r);
        }
        let along = if last { left.min(l) } else { l };
        if let kernel::PathSeg::Arc { a, center, axis, angle } = *s {
            let radius = (a - center).len().max(1e-12);
            let turn = along.copysign(angle) / radius;
            r = crate::mat_mul(&rotation(Vec3::ZERO, axis.normalized().unwrap_or(Vec3::Z), turn), &r);
        }
        let (p, t) = seg_at(s, along);
        if last {
            return (p, r);
        }
        came_along = Some(t);
        left -= l;
    }
    (Vec3::ZERO, r)
}

/// The tangent `d` mm along a chain (clamped to its ends).
fn path_tangent(segs: &[kernel::PathSeg], d: f64) -> Vec3 {
    let mut left = d.max(0.0);
    for (i, s) in segs.iter().enumerate() {
        let l = seg_len(s);
        if left <= l + 1e-9 || i + 1 == segs.len() {
            return seg_at(s, left.min(l)).1;
        }
        left -= l;
    }
    Vec3::X
}

/// The least rotation that turns unit direction `a` onto `b`; a half turn about `normal` (or
/// any line square to `a`) when they are opposite.
fn least_turn(a: Vec3, b: Vec3, normal: Vec3) -> crate::Mat {
    let axis = a.cross(b);
    let (sin, cos) = (axis.len(), a.dot(b));
    if sin > 1e-12 {
        return rotation(Vec3::ZERO, axis, sin.atan2(cos));
    }
    if cos >= 0.0 {
        return crate::IDENTITY;
    }
    let square = |k: Vec3| k.cross(a).len() > 0.5;
    let k = [normal, Vec3::Z, Vec3::X].into_iter().find(|k| square(*k)).unwrap_or(Vec3::Y);
    rotation(Vec3::ZERO, k, std::f64::consts::PI)
}

/// The placement that carries a body from the path's frame `r0` at `p0` to its frame `rk` at
/// `pk`, keeping the body's relation to the path.
fn frame_to_frame(p0: Vec3, r0: &crate::Mat, pk: Vec3, rk: &crate::Mat) -> crate::Mat {
    // The frames are rotations, so r0's inverse is its transpose.
    let mut back = crate::IDENTITY;
    for (c, col) in back.iter_mut().enumerate().take(3) {
        for (row, cell) in col.iter_mut().enumerate().take(3) {
            *cell = r0[row][c];
        }
    }
    let mut m = crate::mat_mul(rk, &back);
    let moved = pk - crate::apply_vector(&m, p0);
    m[3] = [moved.x, moved.y, moved.z, 1.0];
    m
}

/// The bodies a hole cuts: those its feature names, else its component's and its own (as
/// `apply_traced` picks a cut's targets).
fn hole_targets<'a>(st: &'a ModelState, f: &Feature) -> Vec<&'a ModelBody> {
    let all = f.participants.len() == 1 && f.participants.first().is_some_and(|t| t == "*");
    st.bodies
        .iter()
        .filter(|b| {
            if all {
                !b.envelope
            } else if f.participants.is_empty() {
                b.in_default_scope(f)
            } else {
                f.participants.contains(&b.name)
            }
        })
        .collect()
}

/// Whether `new` has a face on a surface no face of `old` lies on. A plane, cylinder, cone,
/// sphere or torus is compared by its geometry, to within a ten-millionth of the bodies' size;
/// any other surface (a spline) by its exact definition in the record. A cut that removes
/// material usually leaves such a face (a wall of the pocket); one that only splits faces where
/// its tool touches them (#18825) leaves none. It is a quick test, not a proof: a cut whose new
/// faces all lie on surfaces the body had already (a thin wall cut away between two pockets)
/// leaves none either, so the caller compares volumes when it finds none.
fn has_a_new_surface(old: &Body, new: &Body) -> bool {
    new_surface_faces(old, new).into_iter().any(|f| f)
}

/// For each face of `new`, whether it lies on a surface no face of `old` lies on, compared as
/// [`has_a_new_surface`] compares them.
fn new_surface_faces(old: &Body, new: &Body) -> Vec<bool> {
    use kernel::FaceSurface as S;
    let tol = (old.size().max(new.size()) * 1e-7).max(1e-7);
    let parallel = |a: Vec3, b: Vec3| a.cross(b).len() <= 1e-9;
    let same_axis = |o: Vec3, a: Vec3, p: Vec3, b: Vec3| {
        let off = p - o;
        parallel(a, b) && (off - a * off.dot(a)).len() <= tol
    };
    let same = |a: &S, b: &S| match (a, b) {
        (S::Plane { normal: n, point: p }, S::Plane { normal: m, point: q }) => parallel(n.vec(), m.vec()) && (*q - *p).dot(n.vec()).abs() <= tol,
        (S::Cylinder { origin: o, axis: a, radius: r }, S::Cylinder { origin: p, axis: b, radius: s }) => {
            (r - s).abs() <= tol && same_axis(*o, a.vec(), *p, b.vec())
        }
        (S::Cone { apex: o, axis: a, half_angle: h }, S::Cone { apex: p, axis: b, half_angle: k }) => {
            o.dist(*p) <= tol && a.vec().dot(b.vec()) > 1.0 - 1e-12 && (h - k).abs() <= 1e-9
        }
        (S::Sphere { center: c, radius: r }, S::Sphere { center: d, radius: s }) => (r - s).abs() <= tol && c.dist(*d) <= tol,
        (S::Torus { center: c, axis: a, major: r, minor: t }, S::Torus { center: d, axis: b, major: s, minor: u }) => {
            c.dist(*d) <= tol && parallel(a.vec(), b.vec()) && (r - s).abs() <= tol && (t - u).abs() <= tol
        }
        _ => false,
    };
    let (old_rec, new_rec) = (old.record(), new.record());
    let olds: Vec<(Option<S>, Option<&solidcore_record::brep::Surface>)> =
        (0..old.face_count()).map(|fi| (kernel::face_surface(old, fi), old_rec.faces.get(fi).map(|f| &f.surface))).collect();
    (0..new.face_count())
        .map(|fi| match kernel::face_surface(new, fi) {
            Some(surface) => !olds.iter().any(|(o, _)| o.as_ref().is_some_and(|o| same(o, &surface))),
            None => new_rec.faces.get(fi).is_some_and(|f| !olds.iter().any(|(_, o)| *o == Some(&f.surface))),
        })
        .collect()
}

/// Whether hole tool `tool` is clear of every body the hole cuts, by their exact boxes (#10701).
/// Boxes only: a probe boolean for every hole of a large hole pattern would double its work. A
/// body whose exact box is unknown, or one of another component (in another frame), leaves the
/// hole not known to miss.
fn hole_misses(st: &ModelState, f: &Feature, tool: &Body) -> bool {
    let targets = hole_targets(st, f);
    let bounds = tool.outline_bounds();
    if targets.is_empty()
        || targets.iter().any(|b| b.component != f.component)
        || bounds.is_empty()
        || !bounds.min.is_finite()
        || !bounds.max.is_finite()
    {
        return false;
    }
    let tool_box = Some(bounds.inflated(1e-6));
    targets.iter().all(|b| more::boxes_apart(&tool_box, &b.body.exact_bounds().ok()))
}

/// The bodies a pattern's cut copies are checked against for missing them ([`copy_misses`]).
fn miss_scope<'a>(st: &'a ModelState, f: &'a Feature, targets: &'a [String]) -> impl Iterator<Item = &'a ModelBody> + 'a {
    let targets = if targets.is_empty() { &f.participants } else { targets };
    let all = targets.len() == 1 && targets.first().is_some_and(|t| t == "*");
    st.bodies.iter().filter(move |b| {
        if all {
            !b.envelope
        } else if targets.is_empty() {
            b.in_default_scope(f)
        } else {
            targets.contains(&b.name)
        }
    })
}

/// Whether a copy has no material to cut in any of its eligible targets.
fn copy_misses(st: &ModelState, f: &Feature, targets: &[String], tools: &[Traced]) -> bool {
    miss_scope(st, f, targets).all(|b| {
        tools.iter().all(|t| {
            #[cfg(test)]
            cost_tests::count(|c| c.miss_intersects += 1);
            // An inconclusive probe is not a miss: it must not reject an otherwise valid cut.
            matches!(kernel::boolean(&b.body, &t.body, BoolOp::Intersect), Ok(None))
        })
    })
}

/// The most faces the tools of the copies of a pattern may have for the kernel's union to join
/// them ([`one_body_of`]): a union of a body with many faces costs more than the copies it
/// would replace.
const MAX_JOINED_FACES: usize = 16_384;

/// The tools of `copies` as one body, so the kernel is asked about all of them at once (#18396).
///
/// Bodies that are apart join without any intersection work ([`kernel::union_apart`]); the rest
/// are joined by the kernel's union, when they are not too many ([`MAX_JOINED_FACES`]). `None`
/// when they cannot be one body (a mesh body, a union that fails): the copies are then asked
/// about one by one, as before.
fn one_body_of(copies: &[&[Traced]]) -> Option<Body> {
    let bodies: Vec<Body> = copies.iter().flat_map(|c| c.iter().map(|t| t.body.clone())).collect();
    match bodies.len() {
        0 => None,
        1 => Some(bodies[0].clone()),
        _ => {
            if let Some(one) = kernel::union_apart(&bodies).ok().flatten() {
                return Some(one);
            }
            if bodies.iter().map(|b| b.face_count()).sum::<usize>() > MAX_JOINED_FACES {
                return None;
            }
            let operands: Vec<&Body> = bodies.iter().collect();
            kernel::union_all(&operands).ok()
        }
    }
}

/// The copies that the point test cannot place, as the bodies the kernel is asked about: each
/// body holds the tools of some of the copies, so the kernel is asked about many copies at once
/// instead of one copy at a time (#18396). A body with many faces makes every boolean cost
/// seconds, and a design of many patterns pays that a copy at a time.
///
/// The bodies of a group are joined in groups of at most [`joined_tools_limit`] tools, as the
/// tools of a cut are: the kernel's boolean takes far longer for a tool of many pieces than for
/// several of fewer. The copies whose tools cannot be one body (a mesh body, a union that fails)
/// are returned apart: they are asked about one by one, as before.
fn copies_as_bodies<'a>(copies: &[&'a [Traced]]) -> (Vec<(Body, Vec<&'a [Traced]>)>, Vec<&'a [Traced]>) {
    let limit = joined_tools_limit().max(1);
    let mut groups: Vec<(Body, Vec<&'a [Traced]>)> = Vec::new();
    let mut one_by_one: Vec<&'a [Traced]> = Vec::new();
    let mut group: Vec<&'a [Traced]> = Vec::new();
    let mut tools = 0;
    for copy in copies {
        let n = copy.len();
        if n > limit {
            // More tools than a probe may hold: the copy is asked about one tool at a time, as
            // before.
            one_by_one.push(*copy);
            continue;
        }
        if !group.is_empty() && tools + n > limit {
            let taken = std::mem::take(&mut group);
            match one_body_of(&taken) {
                Some(body) => groups.push((body, taken)),
                None => one_by_one.extend(taken),
            }
            tools = 0;
        }
        group.push(*copy);
        tools += n;
    }
    if !group.is_empty() {
        let taken = std::mem::take(&mut group);
        match one_body_of(&taken) {
            Some(body) => groups.push((body, taken)),
            None => one_by_one.extend(taken),
        }
    }
    (groups, one_by_one)
}

/// What a pattern's cut shows of one of its copies ([`copies_cut_by_history`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CopyCut {
    /// It took material: a face of its tool is left in a body it cut.
    Took,
    /// It took none: no face of its tool is left anywhere, and no other copy reaches its box.
    Missed,
    /// The cut does not show which.
    Unknown,
}

/// What each of a pattern's cut copies did (by its place among its instances, `instances.len()`
/// of them: instance `first + k + 1` of source feature `src`, its tools `copies[instances[k]]`)
/// to the bodies it cut (`before`, as they were before the cut; `count` bodies in the model
/// then), as the cut's own history shows it (#18396: asking the kernel about every copy was half
/// of a design's rebuild).
///
/// A copy took material when a face of its tool is left in a body after the cut on a surface no
/// face of that body had before: a wall of the pocket the copy made, with material on its other
/// side. A copy took none when no face of its tool is left in any body and no other copy's tool
/// reaches its exact box (grown by the bodies' tolerance): a tool that shares material with a
/// body leaves some of its boundary inside the body (the body is not inside it: it is still
/// there), and that boundary is a face of the result unless another copy cut it away. That holds
/// only for a body of one piece, still one piece after the cut (a tool can take a whole piece of
/// a body of several and leave none of its faces), and for a copy whose tools' faces all trace
/// to that copy. Anything else (a body the history does not cover, a body the cut split, a body
/// of several pieces, a copy beside another or not traced) is not known from it.
fn copies_cut_by_history(
    st: &ModelState,
    before: &[ModelBody],
    count: usize,
    src: u64,
    first: usize,
    copies: &[Traced],
    instances: &[std::ops::Range<usize>],
) -> Vec<CopyCut> {
    use solidcraft_kernel::history::FaceOrigin;
    let n = instances.len();
    let mut took = vec![false; n];
    let mut left = vec![false; n];
    // Every body the copies cut is still there, alone, with what it came from.
    let mut covered = st.bodies.len() == count;
    for old in before {
        let Some(now) = st.body(&old.name) else {
            covered = false;
            continue;
        };
        let crate::lineage::Made::Traced(lineage) = &now.made else {
            covered = false;
            continue;
        };
        if lineage.history.faces.len() != now.body.face_count() || old.body.is_mesh() || now.body.is_mesh() {
            covered = false;
            continue;
        }
        // A body of several pieces (or one the cut made so) can lose a whole piece to a copy
        // that leaves no face of its own.
        if old.body.shell_count() != 1 || now.body.shell_count() != 1 {
            covered = false;
        }
        let copy_of: Vec<Option<usize>> = lineage
            .operands
            .iter()
            .map(|op| match op.as_ref() {
                crate::lineage::Operand::CopiedTool { feature, instance, .. } if *feature == src => {
                    instance.checked_sub(first + 1).filter(|k| *k < n)
                }
                _ => None,
            })
            .collect();
        if copy_of.iter().all(Option::is_none) {
            continue;
        }
        let fresh = new_surface_faces(&old.body, &now.body);
        for (face, fresh) in lineage.history.faces.iter().zip(fresh) {
            for origin in &face.origins {
                if let FaceOrigin::Face { operand, .. } | FaceOrigin::Edge { operand, .. } | FaceOrigin::Offset { operand, .. } = *origin
                    && let Some(Some(k)) = copy_of.get(operand)
                {
                    if let Some(l) = left.get_mut(*k) {
                        *l = true;
                    }
                    if fresh
                        && matches!(origin, FaceOrigin::Face { .. })
                        && let Some(t) = took.get_mut(*k)
                    {
                        *t = true;
                    }
                }
            }
        }
    }
    // Each copy's exact box over its tools, grown by twice the tolerance of the bodies it cuts
    // (the tools' own is in [`more::exact_box`]): two copies closer than that may have the sliver
    // between them taken by the kernel as one. None where a box can't be worked out.
    let grow = 2.0 * before.iter().map(|b| b.body.record().tolerance).fold(0.0, f64::max);
    let boxes: Vec<Option<solidcraft_geom::Aabb3>> = instances
        .iter()
        .map(|range| {
            let tools = copies.get(range.clone())?;
            let mut b = solidcraft_geom::Aabb3::EMPTY;
            for t in tools {
                if t.body.is_mesh() {
                    return None;
                }
                b = b.union(&more::exact_box(&t.body)?);
            }
            Some(b.inflated(grow))
        })
        .collect();
    // Whether every face of copy `k`'s tools traces to copy `k` alone: its tools have a lineage,
    // and each of its operands is copy `k` of `src`'s tool.
    let traced = |k: usize| {
        instances.get(k).and_then(|range| copies.get(range.clone())).is_some_and(|tools| {
            !tools.is_empty()
                && tools.iter().all(|t| {
                    t.lineage.as_ref().is_some_and(|l| {
                        l.operands.iter().all(|op| {
                            matches!(op.as_ref(), crate::lineage::Operand::CopiedTool { feature, instance, .. }
                                if *feature == src && *instance == first + k + 1)
                        })
                    })
                })
        })
    };
    (0..n)
        .map(|k| {
            if took[k] {
                return CopyCut::Took;
            }
            let alone =
                boxes[k].is_some() && boxes.iter().enumerate().all(|(j, other)| j == k || (other.is_some() && more::boxes_apart(&boxes[k], other)));
            if covered && !left[k] && alone && traced(k) { CopyCut::Missed } else { CopyCut::Unknown }
        })
        .collect()
}

/// How many of a pattern's cut copies (`instances`, ranges of `copies`) remove no material, and
/// how many copies there are. [`copy_misses`] intersects a copy with every target, which on a
/// body with many holes costs far more than cutting the copy (#9007). A copy that a point test
/// shows takes material from a target ([`shares_material`]) is not a miss, and skips it; only
/// copies the test cannot place (in an earlier hole, on the edge, thinner than its samples) are
/// intersected. The targets don't change while their copies are counted, so each one's exact box
/// is worked out once, not once for every copy, and each copy's tool box once, not once for every
/// target (#7942).
///
/// The copies that are intersected are asked about together (#18396): the tools of all of them
/// are one body, and the kernel is asked about that body once a target, instead of once a copy.
/// A body with many faces makes every boolean cost seconds, and a design of many patterns pays
/// that a copy at a time. What is counted does not change: a copy that removes no material still
/// counts as one, and the copies that do remove material are still found (the targets that take
/// material from a copy are asked about copy by copy, as before).
fn count_copy_misses(
    st: &ModelState,
    f: &Feature,
    targets: &[String],
    copies: &[Traced],
    instances: &[std::ops::Range<usize>],
    axes: &[Vec<(Vec3, Vec3)>],
) -> (usize, usize) {
    let _span = solidcraft_profile::span("doc", "copy_misses");
    let scope: Vec<(&ModelBody, Option<solidcraft_geom::Aabb3>)> =
        miss_scope(st, f, targets).map(|b| (b, (!b.body.is_mesh()).then(|| more::exact_box(&b.body)).flatten())).collect();
    let (mut missed, mut total) = (0, 0);
    // The copies the point test cannot place: they are asked about together.
    let mut unsure: Vec<&[Traced]> = Vec::new();
    for (k, tools) in instances.iter().enumerate().filter_map(|(k, i)| Some((k, copies.get(i.clone())?))) {
        total += 1;
        let lines = axes.get(k).map_or(&[][..], Vec::as_slice);
        let takes = tools.iter().any(|t| {
            let tool_box = (!t.body.is_mesh()).then(|| more::exact_box(&t.body)).flatten();
            scope.iter().any(|(b, target_box)| shares_material(&b.body, target_box.as_ref(), &t.body, tool_box.as_ref(), lines))
        });
        if !takes {
            unsure.push(tools);
        }
    }
    if unsure.is_empty() {
        return (missed, total);
    }
    let (groups, one_by_one) = copies_as_bodies(&unsure);
    // The targets that take material from the copies of a group: those copies are asked about
    // one by one, as before.
    let mut taking: Vec<Vec<usize>> = Vec::with_capacity(groups.len());
    for (group, _) in &groups {
        let mut hits = Vec::new();
        for (i, (b, _)) in scope.iter().enumerate() {
            #[cfg(test)]
            cost_tests::count(|c| c.miss_intersects += 1);
            if !matches!(kernel::boolean(&b.body, group, BoolOp::Intersect), Ok(None)) {
                hits.push(i);
            }
        }
        taking.push(hits);
    }
    // No target takes material from any of the copies: they all remove no material.
    if taking.iter().all(|t| t.is_empty()) && one_by_one.is_empty() {
        return (missed + unsure.len(), total);
    }
    // The copies whose tools could not be one body are asked about one by one, as before.
    for tools in one_by_one {
        if copy_misses(st, f, targets, tools) {
            missed += 1;
        }
    }
    // Some target takes material from the copies of a group: they are asked about one by one,
    // for those targets only.
    for ((_, group), hits) in groups.iter().zip(&taking) {
        for tools in group {
            let removes = hits.iter().all(|&i| {
                let (b, _) = scope[i];
                tools.iter().all(|t| {
                    #[cfg(test)]
                    cost_tests::count(|c| c.miss_intersects += 1);
                    // An inconclusive probe is not a miss: it must not reject an otherwise valid cut.
                    matches!(kernel::boolean(&b.body, &t.body, BoolOp::Intersect), Ok(None))
                })
            });
            if removes {
                missed += 1;
            }
        }
    }
    (missed, total)
}

/// Whether some point lies in the material of both B-rep bodies `target` and `tool`, by their
/// exact point classification (a point on either's boundary does not count). Tried along
/// `lines` (a hole's axis: through the wall it drills into, however thin and slanted), along the
/// axes of the tool's round faces (a cylinder's axis, the middle of a slot's ends) and along the
/// three lines parallel to the axes through the middle of the box both exact boxes hold (the
/// axis of a straight hole through a plate), within that box. `target_box` is `target`'s exact
/// box and `tool_box` `tool`'s ([`more::exact_box`]), each worked out once by the caller; none,
/// as when it can't be worked out, is no answer. `false` says nothing.
/// The points of a line are classified in one call for the tool and one for the target
/// ([`classified_inside`]), not one call a point: the kernel makes the body's copy and its
/// search structures once for all the points of a call.
fn shares_material(
    target: &Body,
    target_box: Option<&solidcraft_geom::Aabb3>,
    tool: &Body,
    tool_box: Option<&solidcraft_geom::Aabb3>,
    lines: &[(Vec3, Vec3)],
) -> bool {
    if target.is_mesh() || tool.is_mesh() {
        return false;
    }
    let (Some(a), Some(b)) = (target_box, tool_box) else { return false };
    let both = a.intersection(b);
    if both.is_empty() {
        return false;
    }
    let c = both.center();
    let mut all = lines.to_vec();
    all.extend(round_face_axes(tool, MAX_PROBED_FACES));
    all.extend([(c, Vec3::X), (c, Vec3::Y), (c, Vec3::Z)]);
    all.iter().any(|&(o, d)| {
        let Some((lo, hi)) = line_in_box(&both, o, d) else { return false };
        let points: Vec<Vec3> = (0..SAMPLES_PER_LINE).map(|k| o + d * (lo + (hi - lo) * (k as f64 + 0.5) / SAMPLES_PER_LINE as f64)).collect();
        // The points of the line are classified in one call for the tool and one for the
        // target: the kernel makes the body's copy and its search structures once for all
        // the points of a call, and again for every call a point makes on its own (#11257:
        // the six copies of the pattern of the report asked for 130 classifications, against
        // ten).
        let in_tool = classified_inside(tool, &points);
        let wanted: Vec<Vec3> = points.iter().zip(&in_tool).filter(|&(_, &b)| b).map(|(p, _)| *p).collect();
        if wanted.is_empty() {
            return false;
        }
        classified_inside(target, &wanted).iter().any(|&b| b)
    })
}

/// Which of `points` lie in the material of `body`: the kernel classifies them all in one
/// call, which makes the body's copy and its search structures once for all of them. The
/// points are classified one by one when the kernel refuses them all together, as before.
fn classified_inside(body: &Body, points: &[Vec3]) -> Vec<bool> {
    match body.classify_points(points) {
        Ok(classes) => {
            #[cfg(test)]
            cost_tests::count(|c| c.classifications += 1);
            classes.into_iter().map(|c| c == kernel::PointClass::Inside).collect()
        }
        Err(_) => {
            #[cfg(test)]
            cost_tests::count(|c| c.classifications += points.len());
            points.iter().map(|p| matches!(body.classify_point(*p), Ok(kernel::PointClass::Inside))).collect()
        }
    }
}

/// Points tried on each line by [`shares_material`].
const SAMPLES_PER_LINE: usize = 16;
/// Most faces of a tool whose surfaces [`shares_material`] looks along.
const MAX_PROBED_FACES: usize = 32;

/// The axes of the round faces among the first `max` faces of `body`, from their exact surfaces:
/// a cylinder's, a cone's and a torus's axis, and the lines along X, Y and Z through a sphere's
/// centre.
fn round_face_axes(body: &Body, max: usize) -> Vec<(Vec3, Vec3)> {
    let mut lines = Vec::new();
    for face in 0..body.face_count().min(max) {
        match kernel::face_surface(body, face) {
            Some(kernel::FaceSurface::Cylinder { origin, axis, .. }) => lines.push((origin, Vec3::from(axis))),
            Some(kernel::FaceSurface::Cone { apex, axis, .. }) => lines.push((apex, Vec3::from(axis))),
            Some(kernel::FaceSurface::Torus { center, axis, .. }) => lines.push((center, Vec3::from(axis))),
            Some(kernel::FaceSurface::Sphere { center, .. }) => lines.extend([(center, Vec3::X), (center, Vec3::Y), (center, Vec3::Z)]),
            Some(kernel::FaceSurface::Plane { .. }) | None => {}
        }
    }
    lines
}

/// The stretch of the line `o + t d` inside `b`, as the range of `t`; `None` when it misses.
fn line_in_box(b: &solidcraft_geom::Aabb3, o: Vec3, d: Vec3) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
    for (o, d, min, max) in [(o.x, d.x, b.min.x, b.max.x), (o.y, d.y, b.min.y, b.max.y), (o.z, d.z, b.min.z, b.max.z)] {
        if d.abs() < 1e-12 {
            if o < min || o > max {
                return None;
            }
        } else {
            let (a, c) = ((min - o) / d, (max - o) / d);
            lo = lo.max(a.min(c));
            hi = hi.min(a.max(c));
        }
    }
    (lo.is_finite() && hi.is_finite() && lo <= hi).then_some((lo, hi))
}

/// The deepest a pattern or mirror reaches through patterns and mirrors of features (#395).
const MAX_NESTING: usize = 8;

/// The feature a pattern or mirror names as a source. Seeds are keyed by id (#1134): a
/// reference that names no feature dangles, as a missing name did before.
fn source_feature(doc: &Document, id: FeatureId) -> Result<&Feature> {
    doc.feature(id.0).ok_or_else(|| DocError::Unknown(format!("feature `{id}`")))
}

/// How a source feature shows in a warning: its name, or its id when it names no feature.
fn source_label(doc: &Document, id: FeatureId) -> String {
    doc.feature(id.0).map(|f| f.name.clone()).unwrap_or_else(|| id.to_string())
}

/// What a pattern or mirror of features copies, when it is the source of another one (#395): its
/// own sources, and the transforms of its copies. `None` for any other feature, and for a pattern
/// or mirror of bodies (its copies are bodies, which a pattern or mirror of features can't take).
fn copied_features(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, src: &Feature) -> Result<Option<(Vec<FeatureId>, Vec<Mat>)>> {
    match &src.kind {
        FeatureKind::Pattern { features, bodies, components, .. } if bodies.is_empty() && components.is_empty() => {
            Ok(Some((features.clone(), own_copies(doc, vals, st, src)?)))
        }
        FeatureKind::Mirror { features, bodies, .. } if bodies.is_empty() => Ok(Some((features.clone(), own_copies(doc, vals, st, src)?))),
        _ => Ok(None),
    }
}

/// Resolve the ordered identity domain of a new plural Thread using only input-state metadata.
/// Legacy threads have no domain. Repeated bodies are intentional: several faces may share one body.
fn thread_target_bodies<'a>(doc: &Document, st: &'a ModelState, kind: &FeatureKind) -> Result<Option<Vec<&'a ModelBody>>> {
    let FeatureKind::Thread { target_bodies, additional_faces, .. } = kind else { return Ok(None) };
    if target_bodies.is_empty() {
        return Ok(None);
    }
    if target_bodies.len() != 1 + additional_faces.len() || target_bodies.len() > 1024 {
        return Err(DocError::Invalid("a Thread target body is required for each selected face".into()));
    }
    let mut owner = None;
    let mut bodies = Vec::with_capacity(target_bodies.len());
    for name in target_bodies {
        let mut matches = st.bodies.iter().filter(|body| body.name == name.as_ref());
        let body = matches
            .next()
            .filter(|_| matches.next().is_none())
            .ok_or_else(|| DocError::Invalid(format!("Thread target body `{name}` is missing or ambiguous")))?;
        let component = doc.model_body_component(body);
        if owner.is_some_and(|owner| owner != component) {
            return Err(DocError::Invalid(
                "all faces of one Thread feature must belong to the same component; create a separate Thread feature for each component".into(),
            ));
        }
        owner = Some(component);
        bodies.push(body);
    }
    Ok(Some(bodies))
}

/// The copies a pattern or mirror of `features` at `mats` makes, source by source: the source
/// feature, the transforms of its copies, and the instance number of its first copy less one.
/// A source that is itself a pattern or mirror of features (#395) stands for what it copied: its
/// own sources, at each of its transforms moved by each of ours. Those copies are numbered after
/// `mats`' own (which keep the numbers they always had), each nested source in a block of its
/// own, so no two copies of one feature share an instance. Suppressed sources copy nothing.
/// `f` with each face point moved onto its named face (on the body the feature names, when it
/// names one and the face is there) in `state`, and notes on how the faces were found. `None`
/// when `f` names no face; an error when a name no longer finds its face (the feature asks for
/// it to be picked again), or when the frame the faces are looked up in cannot be made. A hole
/// placed by length expressions is looked up where they evaluate in `vals`, not where it was made.
fn with_named_face_points(doc: &Document, vals: &BTreeMap<String, Value>, state: &ModelState, f: &Feature) -> Result<Option<(Feature, Vec<String>)>> {
    thread_target_bodies(doc, state, &f.kind)?;
    // Cylindrical emboss resolves its named face in the selected body's frame
    // and certifies an analytic anchor; mesh projection here would undo that.
    let explicit_emboss = matches!(&f.kind, FeatureKind::Emboss { face: Some(_), .. });
    if explicit_emboss || !f.face_names.iter().any(|n| !n.is_empty()) {
        return Ok(None);
    }
    let mut g = f.clone();
    let mut notes = Vec::new();
    // A hole's position is a place on its face, not a pick of the face: it follows
    // the face along the hole's axis and never slides sideways to the face's nearest
    // point when the face no longer reaches it.
    let hole_axis = match &f.kind {
        FeatureKind::Hole { direction, points: None | Some(crate::SketchPoints { single: true, .. }), .. } => direction.normalized(),
        _ => None,
    };
    let placed = match &f.kind {
        FeatureKind::Hole { position, at: at @ Some(_), .. } => hole_position(vals, position, at).ok(),
        _ => None,
    };
    let fst = face_point_state(doc, state, &f.kind, f.component)?;
    let normal = extrude_normal(doc, state, &f.kind, f.component);
    for (k, (p, n)) in g.kind.face_points_mut().into_iter().zip(&f.face_names).enumerate() {
        if n.is_empty() {
            continue;
        }
        let from = if k == 0 { placed.unwrap_or(*p) } else { *p };
        let scoped;
        let input = if let Some(body) = f.kind.thread_target_body(k) {
            scoped = ModelState { bodies: fst.bodies.iter().filter(|b| b.name == body).cloned().collect(), ..ModelState::default() };
            &scoped
        } else {
            &fst
        };
        let body = f.kind.thread_target_body(k).or_else(|| f.kind.face_body());
        // Only a thread may find a numbered piece of a round side again as its whole face (#16602).
        let hit = if matches!(f.kind, FeatureKind::Thread { .. }) {
            crate::name_lookup::locate_thread_face_on_body(input, body, n, from)
        } else {
            crate::name_lookup::locate_face_on_body(input, body, n, from)
        };
        match hit {
            // A vertex or an edge point saved with the name of a wall along the extrusion, before
            // such points named none (#12968), stays where it is: the name would move it into the
            // wall. A point named after a face square to the extrusion follows that face.
            Some(hit)
                if normal.is_some_and(|d| plane_along(&fst, (hit.body, hit.face), d) && on_edge_without_cap(&fst, (hit.body, hit.face), *p, d)) => {}
            Some(hit) => {
                if let Some(note) = hit.note(n) {
                    notes.push(note);
                }
                match (k, hole_axis) {
                    (0, Some(axis)) => match hit.along(input, from, axis) {
                        Some(q) => *p = q,
                        // The place still lies on the face (the axis may run along it).
                        None if hit.on_point => {}
                        None => notes.push(format!(
                            "the hole's position [{}, {}, {}] is no longer on face `{n}`: it stays where it was placed",
                            from.x, from.y, from.z
                        )),
                    },
                    _ => *p = hit.pick_point(input, *p),
                }
            }
            // A deleted face is picked by its point as well: when its name is gone (a design saved by
            // an older version names merged faces differently), the face is the one face of the same
            // body that holds the point. With none, or with several (the point on an edge they share),
            // nothing is deleted: the feature asks for the face to be picked again (#14206).
            None if matches!(f.kind, FeatureKind::DeleteFace { .. }) => {
                let mut holders = state
                    .bodies
                    .iter()
                    .filter(|b| !b.body.is_mesh() && f.kind.face_body().is_none_or(|name| b.name == name))
                    .flat_map(|b| crate::appearance::faces_within(b, *p, b.body.size() * 1e-3 + 1e-6));
                if holders.next().is_none() || holders.next().is_some() {
                    return Err(DocError::reselect(n.as_str()));
                }
                notes.push(format!("face `{n}` is no longer named so: the one face at [{}, {}, {}] is used", p.x, p.y, p.z));
            }
            None => return Err(DocError::reselect(n.as_str())),
        }
    }
    Ok(Some((g, notes)))
}

/// The sources a pattern or mirror in component `own` copies, each with its copies' transforms
/// in `own`'s frame and its first instance.
fn copy_plan<'a>(
    doc: &'a Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    own: u64,
    features: &[FeatureId],
    mats: &[Mat],
) -> Result<Vec<(&'a Feature, Vec<Mat>, usize)>> {
    #[allow(clippy::too_many_arguments)]
    fn walk<'a>(
        doc: &'a Document,
        vals: &BTreeMap<String, Value>,
        st: &ModelState,
        own: u64,
        features: &[FeatureId],
        mats: &[Mat],
        first: usize,
        next: &mut usize,
        depth: usize,
        out: &mut Vec<(&'a Feature, Vec<Mat>, usize)>,
    ) -> Result<()> {
        // The copies are made in timeline order, whatever order the features were picked in, a
        // pattern's or mirror's own too: a chamfer picked before the hole it blends would
        // otherwise find no edge to copy.
        let mut features = features.to_vec();
        features.sort_by_cached_key(|id| doc.features.iter().position(|x| x.id == id.0).unwrap_or(usize::MAX));
        features.dedup();
        for id in &features {
            if doc.feature(id.0).is_some_and(|x| x.suppressed) {
                continue;
            }
            let src = source_feature(doc, *id)?;
            let Some((inner, inner_mats)) = copied_features(doc, vals, st, src)? else {
                out.push((src, mats.to_vec(), first));
                continue;
            };
            // Made in the inner pattern's component's frame, composed in `own`'s (#19451).
            let inner_mats = match component_in(doc, own, src.component).and_then(|at| crate::mat_inverse(&at)) {
                Some(back) => inner_mats.iter().map(|m| moved_in(m, &back)).collect::<Result<Vec<_>>>()?,
                None => inner_mats,
            };
            if depth >= MAX_NESTING {
                return Err(DocError::Invalid(format!("patterns and mirrors of patterns and mirrors nest at most {MAX_NESTING} deep")));
            }
            // Counted before they are made: a pattern of a pattern multiplies the two counts.
            let count = mats.len().saturating_mul(inner_mats.len());
            if count > MAX_INSTANCES || next.saturating_add(count) > MAX_INSTANCES * MAX_NESTING {
                return Err(DocError::Invalid("too many pattern instances".into()));
            }
            let moved: Vec<Mat> = mats.iter().flat_map(|m| inner_mats.iter().map(move |mi| crate::mat_mul(m, mi))).collect();
            let block = *next;
            *next += moved.len();
            walk(doc, vals, st, own, &inner, &moved, block, next, depth + 1, out)?;
        }
        Ok(())
    }
    let mut next = mats.len();
    let mut out = Vec::new();
    walk(doc, vals, st, own, features, mats, 0, &mut next, 0, &mut out)?;
    Ok(out)
}

/// The transforms of a pattern's or mirror's own copies, in its component's frame.
fn own_copies(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature) -> Result<Vec<Mat>> {
    match &f.kind {
        FeatureKind::Pattern { pattern, .. } => pattern_transforms(doc, vals, st, f.component, pattern),
        FeatureKind::Mirror { plane, .. } => Ok(vec![mirror_copy(doc, vals, st, f, plane)?]),
        _ => Ok(Vec::new()),
    }
}

/// The transform of mirror `f`'s one copy, across `plane`, in its component's frame.
fn mirror_copy(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature, plane: &PlaneRef) -> Result<Mat> {
    Ok(mirror_matrix(&doc.resolve_plane_in(vals, plane, 0, f.component, Some(st))?))
}

/// The transforms of a pattern's or mirror's copies in their instance order (instance `k + 1`
/// is the `k`-th): `mats`, then those of the copies made through patterns and mirrors among
/// `features` ([`copy_plan`]). Naming finds a copy's faces through them.
fn copy_instances(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    f: &Feature,
    features: &[FeatureId],
    mats: &[Mat],
) -> Result<Vec<Mat>> {
    let plan = copy_plan(doc, vals, st, f.component, features, mats)?;
    let len = plan.iter().map(|(_, ms, first)| first + ms.len()).max().unwrap_or(0).max(mats.len());
    let mut all = vec![crate::IDENTITY; len];
    for (slot, m) in all.iter_mut().zip(mats) {
        *slot = *m;
    }
    for (_, ms, first) in &plan {
        for (k, m) in ms.iter().enumerate() {
            if let Some(slot) = all.get_mut(first + k) {
                *slot = *m;
            }
        }
    }
    Ok(all)
}

/// Blend `src` (a fillet or chamfer) as its copy placed by `m` reads it: the points its radius
/// law starts from and its corner setbacks' vertices moved with its edges.
fn moved_blend(src: &Feature, m: &Mat) -> Feature {
    let mut copy = src.clone();
    if let FeatureKind::Fillet { style, setbacks, .. } = &mut copy.kind {
        if let crate::FilletStyle::Variable { start, .. } | crate::FilletStyle::Spline { start, .. } = style {
            start.0 = crate::apply_point(m, start.0);
        }
        for c in setbacks {
            c.vertex.0 = crate::apply_point(m, c.vertex.0);
        }
    }
    copy
}

/// Apply transformed copies of the tools of `features` (patterns, mirrors).
fn replay(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &mut ModelState,
    features: &[FeatureId],
    mats: &[Mat],
    warning: &mut Option<String>,
) -> Result<()> {
    if features.is_empty() {
        return Err(DocError::Invalid("no features selected".into()));
    }
    // A suppressed feature has nothing to copy: its copies go with it.
    let suppressed: Vec<FeatureId> = features.iter().copied().filter(|n| doc.feature(n.0).is_some_and(|x| x.suppressed)).collect();
    if !suppressed.is_empty() {
        let list = suppressed.iter().map(|n| format!("`{}`", source_label(doc, *n))).collect::<Vec<_>>().join(", ");
        *warning = Some(if suppressed.len() == features.len() {
            format!("nothing to copy: {list} suppressed")
        } else {
            format!("{list} suppressed and not copied")
        });
    }
    let (mut missed, mut total) = (0usize, 0usize);
    let own = f.component;
    let plan = copy_plan(doc, vals, st, own, features, mats)?;
    // A source's named faces are looked up in the model as it was before any copy was made: the
    // copy of an earlier source (a boss joined to the body) changes the faces a later source (a cut
    // starting on the boss) names, though the later source means the original's (#11792).
    // A hole placed from edges names those edges too (#16799): the second hole's edge names
    // mean the original's edges, which the first hole's copy has already cut into.
    let named_from = plan.iter().any(|(src, _, _)| src.face_names.iter().chain(&src.edge_names).any(|n| !n.is_empty())).then(|| st.clone());
    for (src, mats, first) in &plan {
        // A fillet or chamfer repeats at the copied edges.
        if let FeatureKind::Fillet { edges, radius, .. } | FeatureKind::Chamfer { edges, distance: radius, .. } = &src.kind {
            let r = val(vals, radius, Kind::Length)?;
            // A fillet's own per-edge radii, in the order of its picks (as it saved its edges).
            let each_r: Option<Vec<f64>> = match &src.kind {
                FeatureKind::Fillet { edge_radii, .. } if !edge_radii.is_empty() => {
                    Some(edge_radii.iter().map(|e| val(vals, e, Kind::Length)).collect::<Result<_>>()?)
                }
                _ => None,
            };
            // The edges the feature blended in this evaluation (found by name), falling back to
            // its picked points.
            let found = st.blend_edges.get(&src.id).cloned().unwrap_or_else(|| crate::plain(edges));
            let picks = PickFrames::new(doc, st, own, src);
            for m in mats {
                // Each edge's copy, in the frame of the body it lands on.
                let each: Vec<(Mat, Option<usize>)> = found.iter().map(|p| picks.copy(st, m, *p, edge_through)).collect::<Result<_>>()?;
                let moved_all: Vec<Vec3> = found.iter().zip(&each).map(|(p, (m, _))| crate::apply_point(m, *p)).collect();
                // The copies of a blend over several bodies blend each body's own edges.
                let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
                for (i, p) in moved_all.iter().enumerate() {
                    let ti = match each.get(i).and_then(|(_, on)| *on).or_else(|| body_on_edge(st, *p)) {
                        Some(ti) => ti,
                        None => blend_target(st, &None, &moved_all)?,
                    };
                    match groups.iter_mut().find(|(b, _)| *b == ti) {
                        Some((_, idx)) => idx.push(i),
                        None => groups.push((ti, vec![i])),
                    }
                }
                for (ti, idx) in groups {
                    let moved: Vec<Vec3> = idx.iter().filter_map(|i| moved_all.get(*i).copied()).collect();
                    let m = idx.first().and_then(|i| each.get(*i)).map_or(m, |(m, _)| m);
                    let Some(mb) = st.bodies.get(ti).cloned() else { continue };
                    let nb = match (&src.kind, st.fitted.get(&src.id)) {
                        // A copy is congruent to the original: the radius it was reduced to first.
                        (FeatureKind::Fillet { turn, .. }, Some(used)) => {
                            let used: Vec<f64> = idx.iter().map(|i| used.get(*i).copied().unwrap_or(r)).collect();
                            mb.traced().then(kernel::fillet_each(&mb.body, &moved, &used, turn.kernel())?)
                        }
                        // A copy of a fillet with per-edge radii pairs each copied edge with its radius.
                        (FeatureKind::Fillet { turn, .. }, None) if each_r.is_some() => {
                            // Before its fillet has run (a pattern placed ahead of it): the radii
                            // pair with the picks, so their count is checked as the fillet checks it.
                            let all = each_r.as_deref().unwrap_or(&[]);
                            if all.len() != edges.len() {
                                return Err(DocError::Invalid(format!(
                                    "`edge_radii` gives {} radii for {} picked edges: one per edge, in the order of `edges`",
                                    all.len(),
                                    edges.len()
                                )));
                            }
                            let used: Vec<f64> = idx.iter().map(|i| all.get(*i).copied().unwrap_or(r)).collect();
                            mb.traced().then(kernel::fillet_each(&mb.body, &moved, &used, turn.kernel())?)
                        }
                        // Otherwise the copy is the same blend, with its type, fit, corners and
                        // setbacks, at the copied edges (#16912): a curvature or spline fillet's
                        // copy was a constant-radius round. Its notes are the original's.
                        _ => {
                            let (nb, from, _, _) = blend_raw(vals, &moved_blend(src, m), &mb, &moved, r, &mut None)?;
                            // A copy blends edges the pattern's own copies made: named after the
                            // body so far.
                            from.then(nb)
                        }
                    };
                    if let Some(slot) = st.bodies.get_mut(ti) {
                        *slot = mb.remade(nb);
                    }
                }
            }
            continue;
        }
        // An Offset Face repeats at the copied faces of the body they lie on.
        if let FeatureKind::OffsetFace { faces, distance, body } = &src.kind {
            let d = val(vals, distance, Kind::Length)?;
            let picked: Vec<Vec3> = crate::plain(faces);
            let picks = PickFrames::new(doc, st, own, src);
            let named = |b: &ModelBody, q: Vec3| body.as_ref().is_none_or(|n| *n == b.name).then(|| box_dist(b, q));
            for m in mats {
                let (m, on) = match picked.first() {
                    Some(p) => picks.copy(st, m, *p, named)?,
                    None => (*m, None),
                };
                let moved: Vec<Vec3> = picked.iter().map(|p| crate::apply_point(&m, *p)).collect();
                let i = match on {
                    Some(i) => i,
                    None => body_at(st, body, &moved)?,
                };
                // A distance of 0 leaves the faces where they are (#20496).
                if d.abs() < 1e-9 {
                    continue;
                }
                let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
                let nb = mb.traced().then(kernel::offset_faces(&mb.body, &moved, d)?);
                if let Some(slot) = st.bodies.get_mut(i) {
                    *slot = mb.remade(nb);
                }
            }
            continue;
        }
        // A moved copy is copied again as a body: each instance is the copy the Move made,
        // placed by the pattern (#5931). Frame members are too: each instance is every member
        // the feature made, as it is here, placed by the pattern.
        if matches!(src.kind, FeatureKind::Move { copy: true, .. } | FeatureKind::FrameMember { .. }) {
            let made: Vec<ModelBody> = st.bodies.iter().filter(|b| b.feature == src.id).cloned().collect();
            if made.is_empty() {
                let what = if matches!(src.kind, FeatureKind::FrameMember { .. }) { "member" } else { "copy" };
                return Err(DocError::Invalid(format!("{} made no {what} to repeat", src.name)));
            }
            // Each copy is a body of the pattern's, in its frame: the source taken there first.
            let made_in: Vec<Option<Mat>> = made.iter().map(|b| body_in(doc, own, b)).collect();
            let mut output = 0;
            for (k, m) in mats.iter().enumerate() {
                for (mb, at) in made.iter().zip(&made_in) {
                    let m = at.map_or(*m, |at| crate::mat_mul(m, &at));
                    let moved = moved_copy(mb, kernel::transform_matrix(&mb.body, m)?, first + k + 1);
                    let name = copy_body_name(st, f, output, &mb.name);
                    st.bodies.push(ModelBody::traced_new(name, moved, f.id));
                    output += 1;
                }
            }
            continue;
        }
        // A thread is cut again on the copy of its face. A copy keeps the thread's hand: the
        // mirror of a right-hand M8 hole is a right-hand M8 hole, not a left-hand one.
        if let FeatureKind::Thread { face, target_bodies, .. } = &src.kind {
            let picks = PickFrames::new(doc, st, own, src);
            // Each further face is kept in its own target body's frame, where the thread finds it.
            let further: Vec<PickFrames> = target_bodies.iter().skip(1).map(|n| PickFrames::on_body(doc, st, own, n)).collect();
            let named = |n: Option<&crate::BodyName>, b: &ModelBody, q: Vec3| n.is_none_or(|n| *n == b.name).then(|| box_dist(b, q));
            for each in mats {
                let (m, _) = picks.copy(st, each, face.0, |b, q| named(target_bodies.first(), b, q))?;
                let m = &m;
                let mut copy = (*src).clone();
                copy.id = f.id;
                copy.face_names.clear();
                if let FeatureKind::Thread { face, additional_faces, from, up_to, .. } = &mut copy.kind {
                    face.0 = crate::apply_point(m, face.0);
                    // Keep each explicit target body while moving its picked point.
                    for (i, q) in additional_faces.iter_mut().enumerate() {
                        let at = match further.get(i) {
                            Some(picks) => picks.copy(st, each, q.0, |b, p| named(target_bodies.get(i + 1), b, p))?.0,
                            None => *m,
                        };
                        q.0 = crate::apply_point(&at, q.0);
                    }
                    if let Some(q) = from {
                        q.0 = crate::apply_point(m, q.0);
                    }
                    match up_to {
                        Some(crate::ThreadLimit::Face(q)) => q.0 = crate::apply_point(m, q.0),
                        Some(crate::ThreadLimit::Plane(_)) => {
                            return Err(DocError::Invalid(format!("{} ends at a plane, so it cannot be patterned or mirrored yet", src.name)));
                        }
                        None => {}
                    }
                }
                thread::eval(doc, vals, &copy, st, warning)?;
            }
            continue;
        }
        // Copies are made in their source's tools' frame and meet each target where it is shown,
        // as the source's own tools do (#3383).
        let _frames = ToolFrames::install(doc, src, st);
        let in_tools: Vec<Mat>;
        let mats: &[Mat] = match tools_component(doc, src).and_then(|c| component_in(doc, own, c)) {
            Some(at) => {
                in_tools = mats.iter().map(|m| moved_in(m, &at)).collect::<Result<_>>()?;
                &in_tools
            }
            None => mats,
        };
        let (op, targets, scoped) = copy_targets(st, f, src);
        let f: &Feature = &scoped;
        // A join that merged its targets left only the first of them (the others now live in
        // it): the copies change the bodies still there (#3378). When none is, the names stay
        // and say which body is missing.
        let present: Vec<String> = targets.iter().filter(|t| st.bodies.iter().any(|b| &b.name == *t)).cloned().collect();
        let targets = if present.is_empty() { targets } else { present };
        let mut copies = Vec::new();
        let mut instances = Vec::with_capacity(mats.len());
        // Each hole copy's axes (spot and direction), where the check for copies that miss looks
        // for material first.
        let mut axes: Vec<Vec<(Vec3, Vec3)>> = Vec::new();
        let mut reaches: Vec<Vec<f64>> = Vec::new();
        if let FeatureKind::Hole { diameter, depth, hole, to, .. } = &src.kind {
            // A hole is drilled again at each copy of its spot (found on the surface there), so
            // a blind hole's depth counts from the face at the copy, not from the bottom of the
            // original hole under the original spot. It meets its bodies where its sketch's frame
            // shows them (#17623).
            let from = targets_as_shown(named_from.as_ref().unwrap_or(st), src, &targets)?;
            let (spots, dir) = hole_spots_raw(vals, &from, src)?;
            let prefixes: Vec<_> = spots.iter().map(|spot| spot.point.map(hole_point_role)).collect();
            let shown = if named_from.is_some() { targets_as_shown(st, src, &targets)? } else { from };
            let st: &ModelState = &shown;
            let mesh_scope = hole_mesh_scope(st, if targets.is_empty() { hole_drill_scope(f) } else { &targets });
            // Copies start where the original does (#11538).
            let from_inside = hole_from_inside(doc, src);
            // A hole at sketch points drills into the material, judged at each copy's spots as
            // the source's side was judged at its own (#14050): judged at the source spots, where
            // the source has already left an empty bore, a through hole's copies turned away.
            let sketch_side = match &src.kind {
                FeatureKind::Hole { points: Some(sp), .. } if !sp.single => st.sketch(sp.sketch.0).map(|ss| (ss.plane.normal(), sp.flip)),
                _ => None,
            };
            // A hole whose tools take nothing from the model at its spot beyond its direction,
            // depth and [`HoleReach`] (no drill point or bridge, which look at the material there)
            // has the same tools at every copy with those, moved: they are built once and moved
            // to the next copy, not built again for each one (#7942).
            let movable = matches!(
                hole,
                HoleKind::Simple | HoleKind::Counterbore { tip_angle: None, bridge: None, .. } | HoleKind::Countersink { tip_angle: None, .. }
            );
            let mut built: Option<HoleBuilt> = None;
            // A hole at a picked spot is kept in the frame of the body it lies on.
            let picks = PickFrames::new(doc, st, own, src);
            let scope = hole_drill_scope(src);
            let named = |b: &ModelBody, q: Vec3| (scope.is_empty() || scope.contains(&b.name)).then(|| box_dist(b, q));
            for (k, m) in mats.iter().enumerate() {
                let (m, _) = match spots.first() {
                    Some(spot) => picks.copy(st, m, spot.at, named)?,
                    None => (*m, None),
                };
                let m = &m;
                let d = match sketch_side {
                    Some((n, flip)) => {
                        let moved = spots.iter().map(|spot| crate::apply_point(m, spot.at));
                        let n = crate::apply_vector(m, n).normalized().ok_or_else(|| DocError::Invalid("hole direction".into()))?;
                        let side = sketch_hole_dir(st, moved, n, hole_scope(src));
                        if flip { -side } else { side }
                    }
                    None => crate::apply_vector(m, dir).normalized().ok_or_else(|| DocError::Invalid("hole direction".into()))?,
                };
                let start = copies.len();
                let mut on = Vec::with_capacity(spots.len());
                let mut reach_of = Vec::with_capacity(spots.len());
                for (spot, prefix) in spots.iter().zip(&prefixes) {
                    let at = hole_on_surface_in(st, crate::apply_point(m, spot.at), d, hole_scope(src), from_inside);
                    on.push((at, d));
                    let to = to.map(|t| crate::apply_point(m, t.0));
                    let depth = hole_depth_to(st, &at, d, &to, depth)?;
                    reach_of.push(depth.as_ref().map_or(Ok(f64::INFINITY), |e| val(vals, e, Kind::Length))?);
                    let reach = hole_reach(vals, st, &at, d, diameter, &depth, hole, !from_inside, mesh_scope)?;
                    let tools = match built.as_ref().filter(|b| movable && b.dir == d && b.depth == depth && b.reach.moves_to(&reach)) {
                        Some(b) => {
                            let to_here = translation(at - b.at);
                            b.tools
                                .iter()
                                .map(|t| Ok(Traced { lineage: t.lineage.clone(), body: kernel::transform_matrix(&t.body, to_here)?, recipe: None }))
                                .collect::<Result<Vec<_>>>()?
                        }
                        None => {
                            let tools: Vec<Traced> =
                                hole_tools_reaching(vals, st, &at, d, diameter, &depth, hole, &reach)?.into_iter().map(|(tool, _)| tool).collect();
                            if movable {
                                built = Some(HoleBuilt { at, dir: d, depth: depth.clone(), reach, tools: tools.clone() });
                            }
                            tools
                        }
                    };
                    // Apply the point name after reusing the shape, which may be another point's.
                    copies.extend(tools.into_iter().map(|tool| {
                        let tool = tool.with_part_prefix(prefix.as_deref());
                        Traced { lineage: tool.lineage.map(|l| l.copied(src.id, first + k + 1)), body: tool.body, recipe: None }
                    }));
                }
                instances.push(start..copies.len());
                axes.push(on);
                reaches.push(reach_of);
            }
        } else if let FeatureKind::Rib { .. } = &src.kind {
            // A rib is built again at each copy of its sketch, against the bodies there (#4593):
            // its walls run until they meet the body, which may not lie the same way there. It
            // meets every body where its sketch's frame shows it (#17655).
            let shown = targets_as_shown(st, src, &["*".to_string()])?;
            for (k, m) in mats.iter().enumerate() {
                for body in more::rib_tools(vals, &shown, src, Some(m))? {
                    // Each copy's faces are named as the rib's, with the instance.
                    let t = Traced::unnamed(body);
                    copies.push(Traced { lineage: t.lineage.map(|l| l.copied(src.id, first + k + 1)), body: t.body, recipe: None });
                }
            }
        } else {
            // Each copy's faces are named as the source feature's, with the instance. Its face
            // points follow their named faces, as the feature's own did (#2121): a To Object
            // extrude's copies reach the face where it is now, not where it was picked. They follow
            // them to where the feature found them, before it ran: a cut up to a face it splits has
            // left no one face of that name to find (#15054).
            let moved = match st.named_faces.get(&src.id) {
                Some(points) => {
                    let mut g = (*src).clone();
                    for (p, q) in g.kind.face_points_mut().into_iter().zip(points) {
                        *p = *q;
                    }
                    Some(g)
                }
                None => with_named_face_points(doc, vals, named_from.as_ref().unwrap_or(st), src)?.map(|(g, _)| g),
            };
            let tools = feature_tools_with_constructor(doc, vals, moved.as_ref().unwrap_or(src), st, &mut None, &mut None, &mut None)?;
            for (k, m) in mats.iter().enumerate() {
                let start = copies.len();
                for t in &tools {
                    let recipe = t.recipe.map(|r| moved_recipe(r, m, src.id, first + k + 1));
                    copies.push(t.copied(kernel::transform_matrix(&t.body, *m)?, src.id, first + k + 1).with_recipe(recipe));
                }
                instances.push(start..copies.len());
            }
        }
        // Checked against the bodies the copies cut, in the frames they cut them in, as they were
        // before the cut.
        let miss_check = match op {
            Operation::Cut => {
                let into: Vec<String> = miss_scope(st, f, &targets).map(|b| b.name.clone()).collect();
                let before: Vec<ModelBody> = st.bodies.iter().filter(|b| into.contains(&b.name)).cloned().collect();
                Some((targets_as_shown(st, f, &into)?.into_owned(), before, st.bodies.len(), copies.clone()))
            }
            _ => None,
        };
        // New bodies from a pattern join their source when they touch it (as one feature would).
        if f.body_names.is_empty() {
            apply_traced(doc, st, f, copies, op, &targets)?;
        } else {
            let start = st.bodies.iter().filter(|b| b.feature == f.id).count();
            let mut named = f.clone();
            named.body_names = f.body_names.iter().skip(start).cloned().collect();
            apply_traced(doc, st, &named, copies, op, &targets)?;
        }
        if let Some((shown, before, count, copies)) = miss_check {
            // A copy whose tool left a wall in a target took material: the cut says so itself
            // (#18396: asking the kernel about every copy was half of a design's rebuild). The
            // copies it does not show are asked about as before.
            let cut = copies_cut_by_history(st, &before, count, src.id, *first, &copies, &instances);
            let unsure: Vec<usize> = (0..instances.len()).filter(|k| cut.get(*k).is_none_or(|c| *c == CopyCut::Unknown)).collect();
            let some_instances: Vec<std::ops::Range<usize>> = unsure.iter().filter_map(|k| instances.get(*k).cloned()).collect();
            let some_axes: Vec<Vec<(Vec3, Vec3)>> =
                if axes.is_empty() { Vec::new() } else { unsure.iter().map(|k| axes.get(*k).cloned().unwrap_or_default()).collect() };
            let (m, t) = count_copy_misses(&shown, f, &targets, &copies, &some_instances, &some_axes);
            missed += m + cut.iter().filter(|c| **c == CopyCut::Missed).count();
            total += t + (instances.len() - unsure.len());
        }
        let mut unthreaded = 0;
        for (on, reach) in axes.iter().zip(&reaches) {
            if let Some((_, d)) = on.first() {
                unthreaded += hole_threads(vals, st, src, f.id, on.iter().map(|(at, _)| *at), *d, reach, true)?;
            }
        }
        if unthreaded > 0 {
            append_warning(warning, format!("{unthreaded} hole copies are not threaded: their walls cannot take the thread"));
        }
    }
    if missed > 0 {
        append_warning(warning, format!("{missed} of {total} cut copies remove no material"));
    }
    Ok(())
}

/// What `f`'s copies of `src` do and change: `src`'s operation, its targets, else its participants
/// (a primitive's or a hole's, #1108, #2755), and the feature whose scope takes them when it names
/// none. That is `f`, or `f` in `src`'s component for an untargeted join, cut or intersect where
/// `f`'s component has no body for it (#12996), those `f`'s earlier copies made aside: a join's
/// copies join that component's bodies, as the join did, not new bodies of `f` (#19451).
fn copy_targets<'a>(st: &ModelState, f: &'a Feature, src: &Feature) -> (Operation, Vec<String>, std::borrow::Cow<'a, Feature>) {
    let (op, targets) = feature_op(src);
    let targets = if targets.is_empty() { src.participants.clone() } else { targets };
    let own = matches!(op, Operation::Join | Operation::Cut | Operation::Intersect)
        && targets.is_empty()
        && f.participants.is_empty()
        && src.component != f.component
        && !st.bodies.iter().any(|b| b.in_default_scope(f) && b.feature != f.id);
    let scope = if own { std::borrow::Cow::Owned(Feature { component: src.component, ..f.clone() }) } else { std::borrow::Cow::Borrowed(f) };
    (op, targets, scope)
}

/// The frame `src`'s geometry is kept in, in component `own`'s frame, when it is another
/// (#19451): its tools' component's ([`tools_component`]), a pattern's or mirror's own
/// component's (its copies' transforms), the first of a moved copy's or a frame member's bodies
/// kept in another (each is copied from its own), else that of the body it picked on. Nothing,
/// without measuring a body, when nothing is placed.
fn kept_in(doc: &Document, st: &ModelState, own: u64, src: &Feature) -> Option<Mat> {
    if doc.occurrences.is_empty() && doc.body_offsets.is_empty() {
        return None;
    }
    match (&src.kind, tools_component(doc, src)) {
        (_, Some(c)) => component_in(doc, own, c),
        (FeatureKind::Pattern { bodies, .. } | FeatureKind::Mirror { bodies, .. }, None) if bodies.is_empty() => {
            component_in(doc, own, src.component)
        }
        (FeatureKind::Move { copy: true, .. } | FeatureKind::FrameMember { .. }, None) => {
            st.bodies.iter().filter(|b| b.feature == src.id).find_map(|b| body_in(doc, own, b))
        }
        _ => body_in(doc, own, st.bodies.get(picked_body(doc, st, src)?)?),
    }
}

/// The body `src`'s picks are kept in the frame of: the body it names; else, by box from its
/// first pick, the nearest of the bodies holding it (its own component's first), else of all.
fn picked_body(doc: &Document, st: &ModelState, src: &Feature) -> Option<usize> {
    let (named, p) = match &src.kind {
        FeatureKind::Fillet { edges, body, .. } | FeatureKind::Chamfer { edges, body, .. } => {
            (body.as_deref(), st.blend_edges.get(&src.id).and_then(|e| e.first().copied()).or(edges.first().map(|e| e.0)))
        }
        FeatureKind::OffsetFace { faces, body, .. } => (body.as_deref(), faces.first().map(|q| q.0)),
        FeatureKind::Hole { position, points: None, .. } => (
            match src.participants.as_slice() {
                [one] if one != "*" => Some(one.as_str()),
                _ => None,
            },
            Some(position.0),
        ),
        FeatureKind::Thread { face, target_bodies, .. } => (target_bodies.first().map(|n| &**n), Some(face.0)),
        _ => return None,
    };
    if let Some(n) = named {
        return st.bodies.iter().position(|b| b.name == n);
    }
    let p = p?;
    let rank = |b: &ModelBody, owner: u64| {
        let d = box_dist(b, p);
        (d > (b.body.size() * 1e-3).max(1e-6), owner != src.component, d)
    };
    let ranked = st.bodies.iter().zip(doc.model_body_components(st)).enumerate().filter(|(_, (b, _))| !b.envelope);
    ranked.map(|(i, (b, owner))| (i, rank(b, owner))).min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal)).map(|(i, _)| i)
}

/// Where a pattern's or mirror's copies of a source's picks are made (#19451): the frame the picks
/// are kept in, in the pattern's ([`kept_in`]), and each body's ([`body_in`]). Nothing when that
/// is the pattern's own frame: the copies are made as they were.
struct PickFrames {
    at: Option<Mat>,
    bodies: Vec<Option<Mat>>,
}

impl PickFrames {
    /// For `src`'s copies by a pattern in component `own`; none for a source with drawn tools,
    /// whose copies' transforms are taken into its tools' frame before.
    fn new(doc: &Document, st: &ModelState, own: u64, src: &Feature) -> PickFrames {
        let at = if tools_component(doc, src).is_some() { None } else { kept_in(doc, st, own, src) };
        PickFrames::kept_at(doc, st, own, at)
    }

    /// For picks kept in the frame of body `name`.
    fn on_body(doc: &Document, st: &ModelState, own: u64, name: &str) -> PickFrames {
        let at = st.bodies.iter().find(|b| b.name == name).and_then(|b| body_in(doc, own, b));
        PickFrames::kept_at(doc, st, own, at)
    }

    fn kept_at(doc: &Document, st: &ModelState, own: u64, at: Option<Mat>) -> PickFrames {
        let bodies = if at.is_some() { st.bodies.iter().map(|b| body_in(doc, own, b)).collect() } else { Vec::new() };
        PickFrames { at, bodies }
    }

    /// Copy `m` of the pick `p`: moved in the pick's frame, it lands on the body `lands` finds
    /// nearest, each body measured in its own frame (one in the pick's frame wins a tie). `m` on
    /// into that body's frame, with the body; `m` itself for picks kept in the pattern's frame.
    fn copy(&self, st: &ModelState, m: &Mat, p: Vec3, lands: impl Fn(&ModelBody, Vec3) -> Option<f64>) -> Result<(Mat, Option<usize>)> {
        let Some(at) = &self.at else { return Ok((*m, None)) };
        let m = moved_in(m, at)?;
        let q = crate::apply_point(&m, p);
        let mut best: Option<((f64, bool), usize, Option<Mat>)> = None;
        for (i, (b, frame)) in st.bodies.iter().zip(&self.bodies).enumerate() {
            let to = match frame {
                Some(f) if f == at => None,
                f => match crate::mat_inverse(&f.unwrap_or(crate::IDENTITY)) {
                    Some(back) => Some(crate::mat_mul(&back, at)),
                    None => continue,
                },
            };
            let Some(d) = lands(b, to.map_or(q, |t| crate::apply_point(&t, q))) else { continue };
            if best.as_ref().is_none_or(|(k, _, _)| (d, to.is_some()) < *k) {
                best = Some(((d, to.is_some()), i, to));
            }
        }
        Ok(match best {
            Some((_, i, to)) => (to.map_or(m, |to| crate::mat_mul(&to, &m)), Some(i)),
            None => (m, None),
        })
    }
}

#[path = "model_cache.rs"]
mod model_cache;
#[path = "eval_more.rs"]
mod more;
pub use model_cache::{PendingChecks, SavedModel};
#[path = "eval_cell_grid.rs"]
pub mod cell_grid;
#[path = "eval_frame.rs"]
pub(crate) mod frame_member;
#[path = "eval_release.rs"]
mod release;
#[path = "eval_surface.rs"]
mod surface;

/// Cuts or lists a tapped hole's thread at `spots`, drilled along `dir` as deep as `reaches`
/// (measured before the hole was cut). `id` names the thread's faces. A `copy` whose spot
/// has no bore wall missed the material and is skipped; one whose wall cannot take the thread
/// is left unthreaded and counted in the result.
#[allow(clippy::too_many_arguments)]
fn hole_threads(
    vals: &BTreeMap<String, Value>,
    st: &mut ModelState,
    src: &Feature,
    id: u64,
    spots: impl Iterator<Item = Vec3>,
    dir: Vec3,
    reaches: &[f64],
    copy: bool,
) -> Result<usize> {
    let FeatureKind::Hole { thread: Some(d), thread_depth, diameter, hand, modeled, ease_out, .. } = &src.kind else { return Ok(0) };
    let thread_depth = thread_depth.as_ref().map(|d| val(vals, d, Kind::Length)).transpose()?;
    // Find the bore beyond any entry recess, including shallow and through holes.
    let r = val(vals, diameter, Kind::Length)? / 2.0;
    // A pipe thread's wall must be found: no fallback place on a guessed wall.
    let pipe = crate::parse_tapped_thread(d).is_some_and(|t| {
        matches!(
            t.family,
            crate::TappedThreadFamily::G | crate::TappedThreadFamily::Rp | crate::TappedThreadFamily::Rc | crate::TappedThreadFamily::Npt
        )
    });
    let ease = thread::runout(vals, ease_out)?;
    // Everything that can fail is found before anything is cut or listed, so a copy that
    // cannot take the thread is left unthreaded rather than failing the whole feature.
    let plan = |st: &ModelState, p: Vec3, walls: Vec<(f64, f64, Vec3)>| -> Result<Tapped> {
        if pipe && walls.is_empty() {
            return Err(DocError::Invalid("no cylindrical pipe-thread wall remains".into()));
        }
        // A pipe thread is one taper from the entry: its first wall only.
        let walls: Vec<(f64, f64, Vec3)> = match walls.first() {
            None => vec![(0.0, f64::INFINITY, p + dir.any_perp() * r + dir * (r * 0.5))],
            Some(first) if pipe => vec![*first],
            Some(_) => walls,
        };
        // The thread depth runs from where the first wall starts, through every piece
        // of the wall it reaches (#13505).
        let first = walls.first().map_or(0.0, |w| w.0);
        let stop = thread_depth.map(|d| first + d);
        let ask = thread::ThreadAsk { designation: Some(d), tapped: true, hand: *hand, ..Default::default() };
        if *modeled {
            // Cut from the entry, as deep as `thread_depth` or along the whole wall:
            // at a blind hole's bottom the groove stops clear of it, or fades out
            // before it with a runout. Every piece is found before any is cut.
            let mut on = Vec::new();
            for (lo, hi, wall) in &walls {
                let extent = match stop {
                    None => thread::Extent::Full,
                    Some(s) if s >= *hi => thread::Extent::Full,
                    Some(s) if s > *lo => thread::Extent::Length(s - lo),
                    Some(_) => break,
                };
                // A piece past the first starts at its end nearest the entry.
                let from = if on.is_empty() { p } else { p + dir * *lo };
                let ask = thread::ThreadAsk { modeled: true, extent, from: Some(from), ease_out: ease, ..ask };
                on.push(thread_on(st, id, *wall, &ask)?);
            }
            return Ok(Tapped::Cut(on));
        }
        let mut threads = Vec::new();
        for (lo, _, wall) in &walls {
            if stop.is_some_and(|s| s <= *lo) {
                break;
            }
            threads.push(thread_on(st, id, *wall, &ask)?.info);
        }
        if let Some(depth) = thread_depth {
            let useful = |t: &ThreadInfo| (t.axis_point + t.axis * t.start - p).dot(dir).max((t.axis_point + t.axis * t.end - p).dot(dir));
            let available = threads.last().map_or(0.0, |t| useful(t) - first);
            if depth > available + 1e-9 {
                return Err(DocError::Invalid(format!(
                    "thread depth {} mm exceeds the cylindrical wall's useful length of {} mm; reduce the thread depth or use full thread depth",
                    show_mm(depth),
                    show_mm(available)
                )));
            }
            // Stop `depth` past the first wall's start in the drilling direction,
            // independently of the wall probe's position and the cylinder surface's
            // axis orientation.
            if let (Some(t), Some(s)) = (threads.last_mut(), stop) {
                let past = useful(t) - s;
                if past > 0.0 {
                    if dir.dot(t.axis) > 0.0 {
                        t.end -= past;
                    } else {
                        t.start += past;
                    }
                }
            }
        }
        Ok(Tapped::Listed(threads))
    };
    let mut skipped = 0;
    for (p, reach) in spots.zip(reaches.iter().copied()) {
        let walls = parallel_tapped_wall(st, p, dir, r, reach);
        if copy && walls.is_empty() {
            continue;
        }
        let done = plan(st, p, walls).and_then(|t| match t {
            Tapped::Cut(on) => on.into_iter().try_for_each(|o| thread::cut(st, o, (crate::thread_forms::ThreadTip::None, None, None)).map(drop)),
            Tapped::Listed(threads) => {
                st.threads.extend(threads);
                Ok(())
            }
        });
        match done {
            Err(_) if copy => skipped += 1,
            other => other?,
        }
    }
    Ok(skipped)
}

enum Tapped {
    Cut(Vec<thread::ThreadOn>),
    Listed(Vec<ThreadInfo>),
}

#[path = "eval_thread.rs"]
mod thread;
use thread::thread_on;
pub use thread::{ThreadInfo, metric_thread_sizes, parse_metric_thread, suggested_thread, suggested_thread_in};

/// Height above `plane` (along its normal) of the object at `p` as seen from `centre`: the face
/// there if it is planar and parallel (its plane), else where a ray from `centre` along the
/// normal meets the body there, else the point itself.
pub fn object_height(st: &ModelState, plane: &Plane, centre: Vec3, p: Vec3) -> f64 {
    height_at(st, face_at(st, p, Some(plane.normal())), plane, centre, p)
}

/// What an extrude From a flat face at a slant needs to know about its other end to tell whether
/// it starts on the face ([`level_start_reason`]).
#[derive(Clone, Copy, Debug, Default)]
pub struct ExtrudeEnds {
    /// With To Object: whether it ends on a flat plane ([`to_end_is_flat`]).
    pub to: Option<bool>,
    pub through_all: bool,
    /// A taper other than 0.
    pub tapered: bool,
    /// A second side (`distance2`).
    pub two_sided: bool,
}

/// Why an extrude From the flat face at a slant `slope` (the extrusion along `n`) starts on a
/// plane parallel to the sketch, at the face's level, instead of on the face; `None` when it
/// starts on the face. The build and the extrude window's arrow both decide by this.
fn level_start_reason(slope: &Plane, n: Vec3, ends: &ExtrudeEnds) -> Option<&'static str> {
    match ends.to {
        Some(false) => return Some("To Object is on a face that is not flat, or within 5 degrees of running along the extrusion"),
        Some(true) if ends.two_sided => return Some("To Object is set with a second side"),
        _ => {}
    }
    if ends.through_all && ends.to.is_some() {
        // To Object decides where the extrude ends, as it does without a slanted From face; the
        // extrude keeps starting at the face's level, as it did before To could end it there.
        Some("To Object is set together with Through All")
    } else if ends.through_all {
        Some("Through All is set")
    } else if ends.tapered {
        Some("the extrude is tapered")
    } else if slope.normal().dot(n).abs() < FROM_SLOPE_LEAST_SINE {
        Some("the face is within 5 degrees of running along the extrusion")
    } else {
        None
    }
}

/// Where an extrude From Object at `p` starts above `centre`, along `plane`'s normal, when it
/// starts on a flat face at a slant there: the level at which the line through `centre` along
/// the normal meets the face's plane. `None` when it starts at [`object_height`] instead: on
/// any other face, or for the reasons [`level_start_reason`] gives with the extrude's other end
/// `ends` (`ends.to` is filled in here from `to`, the To pick). The caller rules out a design
/// kept on the old start (`from_level`), which also starts there.
pub fn from_slope_height(st: &ModelState, plane: &Plane, centre: Vec3, p: Vec3, to: Option<Vec3>, ends: ExtrudeEnds) -> Option<f64> {
    let n = plane.normal();
    let slope = slanted_plane(st, face_at(st, p, Some(n))?, n)?;
    let ends = ExtrudeEnds { to: to.map(|q| to_end_is_flat(st, plane, q)), ..ends };
    if level_start_reason(&slope, n, &ends).is_some() {
        return None;
    }
    let m = slope.normal();
    Some(level_along(plane, centre + n * ((slope.origin - centre).dot(m) / n.dot(m))))
}

/// The plane a To Object end `face` (found as the build finds it, `None` for a point or an edge
/// pick that stands for its own level) gives an extrude on `plane`, when it is flat: the face's
/// plane, square to the extrusion or at a slant to it but not within 5 degrees of running along
/// it, or the plane parallel to the sketch through the pick `p`. `None` for any other face. With
/// a flat end, an extrude From a slanted face starts on that face (#18825); with any other, at
/// the face's level.
fn flat_to_end(st: &ModelState, face: Option<(usize, usize)>, plane: &Plane, centre: Vec3, p: Vec3) -> Option<Plane> {
    let n = plane.normal();
    match face {
        Some(face) if square_face_level(st, face, plane).is_none() => {
            slanted_plane(st, face, n).filter(|slope| slope.normal().dot(n).abs() >= FROM_SLOPE_LEAST_SINE)
        }
        face => Some(plane.offset(height_at(st, face, plane, centre, p))),
    }
}

/// Whether a To Object pick at `p` ends an extrude on `plane` on a flat plane ([`flat_to_end`]),
/// with its face found as the build finds one picked there: the face at the point, unless the
/// point is a vertex or an edge point with no face square to the extrusion there, which stands
/// for its own level ([`on_edge_without_cap`]).
pub fn to_end_is_flat(st: &ModelState, plane: &Plane, p: Vec3) -> bool {
    let face = object_face_at_level(st, None, p, plane).filter(|face| !on_edge_without_cap(st, *face, p, plane.normal()));
    flat_to_end(st, face, plane, p, p).is_some()
}

/// How far `q` lies along `plane`'s normal from its origin.
fn level_along(plane: &Plane, q: Vec3) -> f64 {
    (q - plane.origin).dot(plane.normal())
}

/// The level along `plane`'s normal of face `(body, face)` when it is a plane square to the
/// normal, and the face's own tolerance, from solidcore ([`Body::face_square_level`]).
fn square_face_level(st: &ModelState, (bi, fi): (usize, usize), plane: &Plane) -> Option<solidcore_ops::SquareFaceLevel> {
    st.bodies.get(bi)?.body.face_square_level(fi, plane.normal(), plane.origin).ok()?
}

/// [`object_height`] of the point `p` on `face` (`(body index, face index)`, see [`object_face`]).
fn height_at(st: &ModelState, face: Option<(usize, usize)>, plane: &Plane, centre: Vec3, p: Vec3) -> f64 {
    let n = plane.normal();
    let h = |q: Vec3| level_along(plane, q);
    let Some((bi, fi)) = face else { return h(p) };
    let Some(b) = st.bodies.get(bi) else { return h(p) };
    if let Some(square) = square_face_level(st, (bi, fi), plane) {
        return square.level;
    }
    // A curved or slanted face: where the profile's middle meets it.
    let m = b.mesh();
    for dir in [n, n * -1.0] {
        if let Some((t, tri)) = m.raycast(centre, dir)
            && m.tri_face.get(tri).is_some_and(|x| *x as usize == fi)
        {
            return h(centre + dir * t);
        }
    }
    h(p)
}

/// The face a To Object pick is on: the nearest face over every solid body, `(body index,
/// face index)`, within that body's pick tolerance. Not the first body with any face within
/// its tolerance: that is 5 % of the body's size, so a large body beside a small target (a
/// plate under a post, a frame around a pin) would otherwise take a pick made on the target.
fn face_at(st: &ModelState, p: Vec3, normal: Option<Vec3>) -> Option<(usize, usize)> {
    st.bodies
        .iter()
        .enumerate()
        .filter(|(_, b)| !b.body.is_mesh())
        .filter_map(|(bi, b)| {
            let (d, fi) = nearest_object_face(b, p, normal)?;
            (d <= crate::appearance::pick_tolerance(b)).then_some((d, bi, fi))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, bi, fi)| (bi, fi))
}

/// At a shared edge or corner an extrusion's end plane and side wall can be equally near.
/// Prefer the plane square to the extrusion: choosing the wall would instead extend the tool
/// past the whole body and try to trim it, even when the tool never intersects that body.
/// Only settle a distance tie; a nearby plane must not steal a pick on another face.
fn nearest_object_face(b: &ModelBody, p: Vec3, normal: Option<Vec3>) -> Option<(f64, usize)> {
    let nearest = crate::appearance::nearest_face(b, p)?;
    let Some(n) = normal else { return Some(nearest) };
    if nearest.0 > crate::appearance::pick_tolerance(b) {
        return Some(nearest);
    }
    let tol = (b.body.size() * 1e-3).max(1e-3);
    let Ok(faces) = b.body.faces(tol) else { return Some(nearest) };
    let caps: std::collections::BTreeSet<usize> =
        faces.iter().filter(|f| f.plane_normal.is_some_and(|fnrm| fnrm.cross(n).len() < 1e-6)).map(|f| f.index).collect();
    if caps.is_empty() || caps.contains(&nearest.1) {
        return Some(nearest);
    }
    let tie = (b.body.size() * 1e-9).max(1e-7);
    let mesh = b.mesh();
    mesh.triangles
        .iter()
        .zip(&mesh.tri_face)
        .filter(|(_, fi)| caps.contains(&(**fi as usize)))
        .filter_map(|(tri, fi)| {
            let [a, b, c] = mesh.tri(tri)?;
            let d = crate::appearance::closest_on_triangle(p, a, b, c).dist(p);
            (d <= nearest.0 + tie).then_some((d, *fi as usize))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .or(Some(nearest))
}

/// The face a To / From Object point stands for, `(body index, face index)`: the face its
/// stored `name` finds, else the nearest face at the point. The name settles a tie: in an
/// assembly a part of another component may rest on the target and touch it at the point.
/// Evaluation has moved the point onto its named face. A pattern's or mirror's copy keeps the
/// point where the feature found that face before it ran (#15054), and a later feature may have
/// cut the face away there: the copy still ends at the face the feature names, as the feature
/// did, not at a face that later feature left at the point, such as a chamfer's slope (#17692).
fn object_face(st: &ModelState, name: Option<&str>, p: Vec3, normal: Option<Vec3>) -> Option<(usize, usize)> {
    if let Some(n) = name.filter(|n| !n.is_empty())
        && let Some(hit) = crate::naming::locate_face(st, n, p)
    {
        return Some((hit.body, hit.face));
    }
    face_at(st, p, normal)
}

/// The face a From / To Object point stands for ([`object_face`]), unless that face is a plane
/// square to the extrude direction that lies at another level than the point: then the point is
/// on a plane of its own (a construction plane near a face, within the pick tolerance of it), and
/// its level, not the face's, is where the extent starts or ends.
fn object_face_at_level(st: &ModelState, name: Option<&str>, p: Vec3, plane: &Plane) -> Option<(usize, usize)> {
    let face = object_face(st, name, p, Some(plane.normal()))?;
    match square_face_level(st, face, plane) {
        // At the face's own tolerance: the one the kernel takes a point for on that face.
        Some(square) if (square.level - level_along(plane, p)).abs() > square.tolerance => None,
        _ => Some(face),
    }
}

/// Whether `p` lies where a plane parallel to `n` meets another face of the body of `face`, and no
/// plane square to `n` is there: the pick is then a vertex or an edge point, whose level is the
/// end, not the surface of whichever neighbour came first (#12968). Decided on the B-rep: the
/// faces round a vertex of the record at `p`, else those along the edge whose own curve passes
/// through `p`, and those faces' exact planes.
fn on_edge_without_cap(st: &ModelState, face: (usize, usize), p: Vec3, n: Vec3) -> bool {
    let Some(b) = st.bodies.get(face.0) else { return false };
    if b.body.is_mesh() {
        return false;
    }
    let tol = (b.body.size() * 1e-3).max(1e-3);
    let record = b.body.record();
    let vertex_at_p = |v: u32| {
        record.vertices.get(v as usize).is_some_and(|v| {
            let [x, y, z] = v.point;
            Vec3::new(x, y, z).dist(p) <= tol
        })
    };
    let mut edges: Vec<usize> =
        (0..record.edges.len()).filter(|e| record.edges.get(*e).is_some_and(|e| e.vertices.iter().any(|v| vertex_at_p(*v)))).collect();
    if edges.is_empty() {
        match kernel::comb_edge_near(&b.body, p) {
            Ok(Some((d, e))) if d <= tol => edges.push(e),
            _ => return false,
        }
    }
    let near: std::collections::BTreeSet<usize> =
        edges.iter().filter_map(|e| record.edges.get(*e)).flat_map(|e| e.uses.iter().map(|u| u.face as usize)).collect();
    // Only where a plane the extrusion runs along meets another face: the seam of a curved wall,
    // or the corner of two slanted facets, is a place on a surface to run to.
    let normals: Vec<Vec3> = near
        .iter()
        .filter_map(|fi| match kernel::face_surface(&b.body, *fi) {
            Some(kernel::FaceSurface::Plane { normal, .. }) => Some(normal.vec()),
            _ => None,
        })
        .collect();
    near.len() > 1 && normals.iter().any(|fnrm| fnrm.dot(n).abs() < 1e-6) && !normals.iter().any(|fnrm| fnrm.cross(n).len() < 1e-6)
}

/// The body of `face` when that face is not a plane (with `n`: not a plane square to `n`), so To
/// Object must follow its shape.
fn curved_face(st: &ModelState, face: Option<(usize, usize)>, n: Option<Vec3>) -> Option<usize> {
    let (bi, fi) = face?;
    let b = st.bodies.get(bi)?;
    let tol = (b.body.size() * 1e-3).max(1e-3);
    let normal = b.body.faces(tol).ok()?.into_iter().find(|f| f.index == fi).map(|f| f.plane_normal);
    let planar = match n {
        Some(n) => normal.flatten().is_some_and(|fnrm| fnrm.cross(n).len() < 1e-6),
        None => normal.is_some_and(|fnrm| fnrm.is_some()),
    };
    (!planar).then_some(bi)
}

/// Whether a face runs along `n` everywhere, from its exact surface: a plane whose normal is
/// square to `n`, or a cylinder whose axis is parallel to `n` (the side of a prism or a cylinder
/// parallel to `n`), so a sweep along `n` never meets it.
fn runs_along(st: &ModelState, (bi, fi): (usize, usize), n: Vec3) -> bool {
    let Some(b) = st.bodies.get(bi) else { return false };
    let Some(n) = n.normalized() else { return false };
    match kernel::face_surface(&b.body, fi) {
        Some(kernel::FaceSurface::Plane { normal, .. }) => normal.vec().dot(n).abs() < 1e-6,
        Some(kernel::FaceSurface::Cylinder { axis, .. }) => axis.vec().cross(n).len() < 1e-6,
        _ => false,
    }
}

/// Whether a face that runs along the normal of `plane` ([`runs_along`]), seen along that
/// normal, meets one of the profile `regions`. Seen so, the face is the curve its boundary edges
/// make: their exact curves, sampled finely, meet a region where a sample lies in it or a step
/// between two samples crosses one of its loops (a profile narrower than the steps). `growth` is
/// how far a taper grows the profile: the face meets it when it lies that close to it (#10711).
fn meets_footprint(st: &ModelState, (bi, fi): (usize, usize), plane: &Plane, regions: &[Region2], growth: f64) -> bool {
    let Some(b) = st.bodies.get(bi) else { return false };
    let tol = (b.body.size() * 1e-4).max(1e-4);
    let Ok(edges) = b.body.edges(tol) else { return true };
    let own = kernel::face_edge_indices(&b.body, fi);
    let curves: Vec<Vec<Vec2>> = own.iter().filter_map(|e| edges.get(*e)).map(|e| e.points.iter().map(|q| plane.to_local(*q)).collect()).collect();
    if curves.iter().flatten().any(|q| regions.iter().any(|r| r.contains(*q))) {
        return true;
    }
    let loops: Vec<_> = regions.iter().flat_map(|r| std::iter::once(&r.outer).chain(&r.holes)).map(|l| l.polyline(1e-3)).collect();
    if curves.iter().flat_map(|c| c.windows(2)).any(|w| loops.iter().any(|l| l.edges().any(|(a, b)| segments_meet(w[0], w[1], *a, *b)))) {
        return true;
    }
    // A taper that grows the profile reaches a wall the profile itself does not cross: the wall
    // is met when it lies within the growth of the profile (#10711).
    growth > 0.0 && curves.iter().flatten().any(|q| distance_to_regions(*q, regions) <= growth)
}

/// How far `q` lies from the outline of the regions `regions`: 0 inside one of them.
fn distance_to_regions(q: Vec2, regions: &[Region2]) -> f64 {
    if regions.iter().any(|r| r.contains(q)) {
        return 0.0;
    }
    let mut best = f64::INFINITY;
    for r in regions {
        for l in std::iter::once(&r.outer).chain(&r.holes) {
            for (a, b) in l.polyline(1e-3).edges() {
                best = best.min(distance_point_segment(q, *a, *b));
            }
        }
    }
    best
}

/// How far `p` lies from the segment `a`-`b`.
fn distance_point_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let (ux, uy) = (b.x - a.x, b.y - a.y);
    let len2 = ux * ux + uy * uy;
    let t = if len2 <= 0.0 { 0.0 } else { (((p.x - a.x) * ux + (p.y - a.y) * uy) / len2).clamp(0.0, 1.0) };
    let (dx, dy) = (p.x - (a.x + t * ux), p.y - (a.y + t * uy));
    (dx * dx + dy * dy).sqrt()
}

/// Whether segments `p`-`q` and `a`-`b` cross or touch.
fn segments_meet(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> bool {
    let side = |o: Vec2, u: Vec2, v: Vec2| (u.x - o.x) * (v.y - o.y) - (u.y - o.y) * (v.x - o.x);
    let (d1, d2) = (side(a, b, p), side(a, b, q));
    let (d3, d4) = (side(p, q, a), side(p, q, b));
    d1 * d2 <= 0.0 && d3 * d4 <= 0.0 && !(d1 == 0.0 && d2 == 0.0 && d3 == 0.0 && d4 == 0.0)
}

/// The body at `p` when the face there is not planar.
fn curved_target_any(st: &ModelState, p: Vec3) -> Option<usize> {
    curved_face(st, face_at(st, p, None), None)
}

/// Tools run through `target`, cut back to its surface: each tool less the body, keeping the
/// piece(s) that touch the start (`dist_to_start` 0 there).
fn trim_to_body(tools: Vec<Traced>, target: &ModelBody, dist_to_start: impl Fn(Vec3) -> f64) -> Result<Vec<Traced>> {
    let mut out = Vec::new();
    let target_traced = target.traced();
    for t in tools {
        let Some(cut) = kernel::boolean(&t.body, &target.body, BoolOp::Cut)? else { continue };
        let cut = Traced::step(cut, &[&t, &target_traced]);
        let lumps = cut.body.lumps()?;
        let lineages = crate::lineage::Lineage::lumps(&cut.body, cut.lineage.as_ref(), &lumps);
        for (lump, lineage) in lumps.into_iter().zip(lineages) {
            let tol = (lump.size() * 1e-4).max(1e-6);
            let touches = lump
                .mesh(&solidcraft_kernel::MeshTolerance::chord((lump.size() * 1e-2).max(1e-2)))
                .map(|m| m.positions.iter().any(|q| dist_to_start(*q) < tol))
                .unwrap_or(false);
            if touches {
                out.push(Traced { body: lump, lineage, recipe: None });
            }
        }
    }
    if out.is_empty() {
        return Err(DocError::Invalid("nothing lies between the profile and the To object".into()));
    }
    Ok(out)
}

/// Tools run to a curved To face, one construction chosen by the exact on-face test, not by a
/// failed attempt: the kernel's [`kernel::face_split_if_on_face`] splits each tool's prism at the
/// face's extended surface and says whether its section lies wholly on the face. When every
/// tool's does, those pieces (the one that starts at `start`, whether that lies in air or inside
/// the target's material) are the result; otherwise the tool is trimmed to the target body
/// ([`trim_to_body`], as a profile only partly over the face always was). Where a piece still
/// has its end cap at `far`, part of the profile lies beyond the face's edges: the tools then end
/// at the face's surface extended past them ([`trim_to_offset_face`] with no offset), refused
/// when a piece still runs out to `far`. `near` is a point on the face (the To pick).
#[allow(clippy::too_many_arguments)]
fn trim_to_face(tools: Vec<Traced>, target: &ModelBody, face: usize, near: Vec3, plane: &Plane, start: f64, far: f64) -> Result<Vec<Traced>> {
    let mut out = Vec::new();
    let mut on_face = true;
    for t in &tools {
        let Some(parts) = kernel::face_split_if_on_face(&t.body, &target.body, face, near, plane, start, far)? else {
            on_face = false;
            break;
        };
        out.extend(parts.into_iter().map(|part| Traced::step(part, &[t])));
    }
    if !on_face {
        let n = plane.normal();
        let from_start = |q: Vec3| ((q - plane.origin).dot(n) - start).abs();
        let trimmed = trim_to_body(tools.clone(), target, from_start)?;
        if trimmed.iter().any(|t| kernel::has_cap_at(&t.body, plane, far, far - start)) {
            // Part of the profile lies beyond the face's edges. A flat face at a slant then ends
            // the tools at its plane, extended past those edges, or at the target body where
            // they meet its material first, as they do where the profile lies over it.
            if let Some(kernel::FaceSurface::Plane { normal, point }) = kernel::face_surface(&target.body, face)
                && let Some(face_plane) = Plane::from_normal(point, normal.vec())
            {
                let planed = trim_to_plane(tools, &face_plane, plane, start, far)?;
                return trim_to_body(planed, target, from_start);
            }
            // A curved face: the tools end at its surface extended past those edges, as a To
            // offset ends them at its offset surface (#20389).
            let travel = if far > start { n } else { n * -1.0 };
            // Its refusals speak of an offset this extent does not have: refused as before.
            let past = || DocError::Invalid("the profile reaches past the To face".into());
            let surfaced = trim_to_offset_face(tools, target, face, near, travel, 0.0, from_start).map_err(|_| past())?;
            if surfaced.iter().any(|t| kernel::has_cap_at(&t.body, plane, far, far - start)) {
                return Err(past());
            }
            return Ok(surfaced);
        }
        return Ok(trimmed);
    }
    if out.is_empty() {
        return Err(DocError::Invalid("nothing lies between the profile and the To object".into()));
    }
    Ok(out)
}

/// Tools run through `target`, cut back to the surface of its face `face` offset by `offset`
/// along `travel`, the direction the extent runs (a negative offset: that far short of the face):
/// each tool split by the face's offset sheet (`offset_sheet`), extended, keeping the piece(s)
/// that touch the start (a plane face of the piece at `dist_to_start` 0). The kernel refuses the offset for some
/// surfaces (#8137): the feature then fails with that reason rather than ending somewhere else.
/// `near` is a point on the face (the To pick). With no offset, see [`trim_to_face`].
fn trim_to_offset_face(
    tools: Vec<Traced>,
    target: &ModelBody,
    face: usize,
    near: Vec3,
    travel: Vec3,
    offset: f64,
    dist_to_start: impl Fn(Vec3) -> f64,
) -> Result<Vec<Traced>> {
    let target_traced = target.traced();
    let sheet = offset_sheet(target, face, near, travel, offset)?;
    // Split where each tool crosses the sheet the way it does at the pick (#20389).
    let split = tools.iter().map(|t| kernel::split_at_facing_side(&t.body, &sheet, 0, near, travel)).collect::<std::result::Result<Vec<_>, _>>().map_err(|e| {
        DocError::Invalid(format!(
            "not supported yet: To Object with an offset from this face: the extent cannot be cut by its offset surface ({e}); use a Distance, or an offset of 0"
        ))
    })?;
    let mut out = Vec::new();
    for (t, parts) in tools.iter().zip(split) {
        for part in parts {
            let tol = (part.size() * 1e-4).max(1e-6);
            // Touching the start: a plane face square to the travel at the start's level (a piece
            // of the tool's start cap), read from the B-rep.
            let touches = (0..part.face_count()).any(|fi| {
                matches!(kernel::face_surface(&part, fi), Some(kernel::FaceSurface::Plane { normal, point })
                    if normal.vec().cross(travel).len() <= 1e-6 * travel.len() && dist_to_start(point) < tol)
            });
            if touches {
                out.push(Traced::step(part, &[t, &target_traced]));
            }
        }
    }
    if out.is_empty() {
        return Err(DocError::Invalid("nothing lies between the profile and the To object".into()));
    }
    Ok(out)
}

/// Tools run from level `start` along `plane`'s normal out to `far`, through `target`, cut back
/// to its curved To face `face` (`to` is the To pick): to the face itself ([`trim_to_face`]), or
/// with a To offset to the face's offset surface ([`trim_to_offset_face`], #12617).
#[allow(clippy::too_many_arguments)]
fn trim_to_curved_face(
    tools: Vec<Traced>,
    target: &ModelBody,
    face: usize,
    to: Vec3,
    to_offset: f64,
    plane: &Plane,
    start: f64,
    far: f64,
) -> Result<Vec<Traced>> {
    if to_offset.abs() <= 1e-9 {
        return trim_to_face(tools, target, face, to, plane, start, far);
    }
    let n = plane.normal();
    let travel = if far > start { n } else { n * -1.0 };
    let trimmed = trim_to_offset_face(tools, target, face, to, travel, to_offset, |q: Vec3| ((q - plane.origin).dot(n) - start).abs())?;
    // A tool the offset surface misses comes back whole, its end cap still at the far height the
    // extent was run out to: refused, as `trim_to_face` does.
    if trimmed.iter().any(|t| kernel::has_cap_at(&t.body, plane, far, far - start)) {
        return Err(DocError::Invalid("the profile reaches past the offset surface of the To face".into()));
    }
    Ok(trimmed)
}

/// How long a tool drafted by `taper` over `regions`, from level `base` along `sign` times the
/// sketch's normal, runs to go past the curved To face `face` of `target` (`to` is the To pick,
/// `to_offset` its offset), to be cut back to it with [`trim_to_curved_face`] (#15820). No longer
/// than it must: a taper that narrows the profile, or a hole in it, closes it a finite way along,
/// which can lie inside a deep target. An envelope that holds the tool and never closes says how
/// far: the outer loop alone, drafted when the taper grows it (only the holes narrow) and
/// straight when the taper narrows it. Run right through the target (past the far side of its
/// exact box by the margin the straight path takes) and cut back to the face, its far end is the
/// farthest the tool meets the face; the tool runs a hair past that, and at least `len`. The
/// envelope only shortens the tool: when its cut fails for a narrowing taper (a straight envelope
/// wider than the target passes beside it where the tool meets it), the tool runs a hair past the
/// far side of the target's box, beyond which it meets nothing of it, and its own cut says
/// whether it ends on the face. A tool that closes before that is refused when it is built.
#[allow(clippy::too_many_arguments)]
fn tapered_reach(
    ss: &SolvedSketch,
    regions: &[Region2],
    (target, face, to, to_offset): (&ModelBody, usize, Vec3, f64),
    base: f64,
    sign: f64,
    len: f64,
    taper: f64,
    corner: Option<kernel::DraftCorner>,
) -> Result<f64> {
    let start_plane = ss.plane.offset(base);
    let travel = if sign > 0.0 { ss.plane.normal() } else { -ss.plane.normal() };
    // How far a body runs from the start along the travel: the far end of its exact box's span
    // from the start, as the kernel measures it.
    let far_side = |body: &Body| -> Result<f64> {
        let bb = body.exact_bounds().map_err(|e| DocError::Invalid(e.to_string()))?;
        let [_, far] = solidcore_ops::affine::span_along(bb.min.into(), bb.max.into(), start_plane.origin.into(), travel.into())
            .map_err(|e| DocError::Invalid(e.to_string()))?;
        Ok(far)
    };
    let target_far = far_side(&target.body)?.max(len) + to_offset.abs();
    let through = target_far + target.body.size() * 0.05 + 1.0;
    extrude_within_reach([base, base + sign * through])?;
    let mut envelopes = Vec::new();
    for r in regions {
        let outer = Region2 { outer: r.outer.clone(), holes: Vec::new() };
        if taper > 0.0 {
            let corner = match corner {
                Some(c) => c,
                None => draft_corner_for(ss, &outer, through, taper, &mut Vec::new())?,
            };
            envelopes.push(Traced::unnamed(kernel::extrude_tapered_between(&start_plane, &outer, 0.0, through, 0.0, sign, taper, corner)?));
        } else {
            let (lo, hi) = if sign < 0.0 { (base - through, base) } else { (base, base + through) };
            envelopes.extend(centered_extrude(&ss.plane, &outer, lo, hi, None, None, CapRoles::ByHeight, None)?);
        }
    }
    let cut = match trim_to_curved_face(envelopes, target, face, to, to_offset, &ss.plane, base, base + sign * through) {
        Err(_) if taper < 0.0 => return Ok(target_far + target.body.size() * 1e-3),
        cut => cut?,
    };
    let reach = cut.iter().map(|t| far_side(&t.body)).collect::<Result<Vec<_>>>()?.into_iter().fold(len, f64::max);
    Ok(reach + target.body.size() * 1e-3)
}

/// The surface of `target`'s face `face` as a sheet body `offset` past the face along `travel`
/// (short of it when negative): the kernel's offset of that face, whose sign follows the face's
/// surface normal, which for a reversed face (the bore of a tube) points the other way than the
/// travel past it. The sheet is built with the offset's own sign and rebuilt on the other side
/// when it lands on the wrong one, measured on the sheet's exact surface at its point nearest
/// `near`.
fn offset_sheet(target: &ModelBody, face: usize, near: Vec3, travel: Vec3, offset: f64) -> Result<Body> {
    let refused = |e: &dyn std::fmt::Display| {
        DocError::Invalid(format!(
            "not supported yet: To Object with an offset from this face: its offset surface cannot be built ({e}); use a Distance, or an offset of 0"
        ))
    };
    let dir = travel.normalized().ok_or_else(|| DocError::Invalid("the extent has no direction".into()))?;
    let tol = (target.body.size() * 1e-4).clamp(1e-4, 1.0);
    let sheet = kernel::offset_surface(&target.body, &[face], offset, tol).map_err(|e| refused(&e))?;
    // Where the sheet landed, along the travel from the pick (the point of its exact surface
    // nearest the pick): past the face when positive.
    let side = kernel::face_surface(&sheet, 0).and_then(|s| s.nearest(near)).map(|q| (q - near).dot(dir));
    match side {
        Some(side) if side * offset < 0.0 => kernel::offset_surface(&target.body, &[face], -offset, tol).map_err(|e| refused(&e)),
        _ => Ok(sheet),
    }
}
/// The sine of the least angle between a From Object face and the extrusion for the extrude to
/// start on the face (5 degrees): nearer to running along it, the profile's shadow on the face
/// is long and thin, the prism over it many times the extrude's length, and the extrude starts
/// at the face's level instead.
const FROM_SLOPE_LEAST_SINE: f64 = 0.087_155_742_747_658_17;

/// What an edit of extrude `old` into `new` keeps of the way a one-sided extrude went
/// ([`crate::Extent::one_sided_direction`]); `model` holds `old` as last evaluated. A one-sided
/// extrude made symmetric records the way it went, read from its caps' names just after it was
/// built: besides its `start` cap it has a cap named `top` when it went along the sketch normal
/// and `bottom` when it went against it. A symmetric extrude edited and still symmetric keeps
/// what it had. Any other edit records nothing, and so does a one-sided extrude whose caps do not
/// say (one gone, both alike, or the extrude not built): no cap of it will answer to `start`.
pub fn keep_one_sided_direction(model: &Model, old: &Feature, new: &mut Feature) {
    let FeatureKind::Extrude { extent: after, .. } = &mut new.kind else { return };
    let symmetric = |e: &crate::Extent| e.direction == crate::Direction::Symmetric && e.distance2.is_none();
    let FeatureKind::Extrude { extent: before, .. } = &old.kind else {
        after.one_sided_direction = None;
        return;
    };
    after.one_sided_direction = if !symmetric(after) {
        None
    } else if symmetric(before) {
        before.one_sided_direction
    } else if before.distance2.is_some() {
        None
    } else {
        model.result(old.id).filter(|r| !r.skipped && r.error.is_none()).and_then(|r| {
            let has = |role: &str| {
                let name = format!("F{}:{role}", old.id);
                r.state.bodies.iter().any(|b| crate::name_lookup::body_has_face_named(b, &name))
            };
            match (has("start"), has("top"), has("bottom")) {
                (true, true, false) => Some(crate::Direction::Positive),
                (true, false, true) => Some(crate::Direction::Negative),
                _ => None,
            }
        })
    };
}

/// Whether two versions of one extrude start From Object on the same face: the same pick
/// point, or faces on one plane, each found as its build finds it in `st`, the model before the
/// feature: by its face name or, failing that, at its pick point, with the component placements
/// its pick measures against. An edit that keeps the From face on its plane leaves where a
/// design saved before From Object followed a slanted face starts (`from_level`). Faces that are
/// not flat, or not found, don't count.
pub fn same_extrude_from_face(doc: &Document, st: &ModelState, a: &Feature, b: &Feature) -> bool {
    let pick = |f: &Feature| match &f.kind {
        FeatureKind::Extrude { extent, .. } => extent.from,
        _ => None,
    };
    let (Some(pa), Some(pb)) = (pick(a), pick(b)) else { return false };
    if pa.dist(pb.0) < 1e-8 {
        return true;
    }
    // Each face's plane, its unit normal, and the tolerance its body is modelled to.
    let plane = |f: &Feature| -> Option<(Vec3, Vec3, f64)> {
        let FeatureKind::Extrude { extent, .. } = &f.kind else { return None };
        let p = extent.from?;
        let tst = state_in_frame_pinned_to(doc, st, f.component, &extent.picked_placements).ok()?;
        let name = f.face_names.get(usize::from(extent.to.is_some())).map(String::as_str);
        let (bi, fi) = object_face(&tst, name, p.0, None)?;
        let b = tst.bodies.get(bi)?;
        let Some(kernel::FaceSurface::Plane { normal, point }) = kernel::face_surface(&b.body, fi) else { return None };
        Some((point, normal.vec().normalized()?, (b.body.size() * 1e-7).max(1e-7)))
    };
    // One plane: the same normal, and each face's point on the other's plane to within the
    // bodies' tolerance, which depends on their size and not on where they are.
    match (plane(a), plane(b)) {
        (Some((p, m, ta)), Some((q, n, tb))) => m.dot(n) > 1.0 - 1e-9 && (q - p).dot(m).abs() < ta.max(tb),
        _ => false,
    }
}

/// The plane of face `(body, face)` when it is flat and at a slant to `n`: neither square to it
/// nor running along it.
fn slanted_plane(st: &ModelState, (bi, fi): (usize, usize), n: Vec3) -> Option<Plane> {
    let b = st.bodies.get(bi)?;
    let Some(kernel::FaceSurface::Plane { normal, point }) = kernel::face_surface(&b.body, fi) else { return None };
    let across = normal.vec().dot(n).abs();
    (across > 1e-6 && across < 1.0 - 1e-9).then(|| Plane::from_normal(point, normal.vec())).flatten()
}

/// How far `slope` lies along `plane`'s normal over the profiles, measured from `level`: the
/// least and the most, over every point of the regions' outer loops.
fn slope_rise(slope: &Plane, plane: &Plane, regions: &[Region2], level: f64) -> (f64, f64) {
    let n = plane.normal();
    let m = slope.normal();
    let across = n.dot(m);
    let mut rise = (0.0_f64, 0.0_f64);
    for q in regions.iter().flat_map(|r| r.outer.polyline(1e-2)) {
        let q = plane.to_world(q);
        let t = (slope.origin - q).dot(m) / across - level;
        rise = (rise.0.min(t), rise.1.max(t));
    }
    rise
}

/// Each tool's part between `slope` moved `lo` and `hi` along `n` (the extrusion direction).
fn trim_between_slopes(tools: Vec<Traced>, slope: &Plane, n: Vec3, lo: f64, hi: f64) -> Result<Vec<Traced>> {
    let with_normal = n.dot(slope.normal()) > 0.0;
    let moved = |d: f64| Plane { origin: slope.origin + n * d, ..*slope };
    let mut out = Vec::new();
    for t in &tools {
        // The two ends are named as a two-sided extrude's caps are, by the end they close:
        // both may lie on one side of the sketch plane, so their height can't tell them apart.
        let Some(above) = half_space_traced(t, &moved(lo), with_normal, "bottom")? else { continue };
        if let Some(part) = half_space_traced(&above, &moved(hi), !with_normal, "top")? {
            out.push(part);
        }
    }
    if out.is_empty() {
        return Err(DocError::Invalid("nothing lies between the From face and the extrude's end".into()));
    }
    Ok(out)
}

/// Each tool's part past `from` and short of `to` along `n` times `sign` (the way the extrude
/// runs; `n` is the sketch's normal): an extrude From one flat face To another (#18825). The ends
/// are named as [`trim_between_slopes`] names them, `bottom` the one lower along `n`.
fn trim_between_planes(tools: Vec<Traced>, from: &Plane, to: &Plane, n: Vec3, sign: f64) -> Result<Vec<Traced>> {
    let travel = n * sign;
    let (from_cap, to_cap) = if sign > 0.0 { ("bottom", "top") } else { ("top", "bottom") };
    let mut out = Vec::new();
    for t in &tools {
        // Past `from`: the side its normal points to when the travel runs with it. Short of `to`:
        // the side its normal points to when the travel runs against it.
        let Some(past) = half_space_traced(t, from, travel.dot(from.normal()) > 0.0, from_cap)? else { continue };
        if let Some(part) = half_space_traced(&past, to, travel.dot(to.normal()) < 0.0, to_cap)? {
            out.push(part);
        }
    }
    if out.is_empty() {
        return Err(DocError::Invalid("nothing lies between the From face and the To face over the profile".into()));
    }
    Ok(out)
}

/// Whether `to` lies before `from` along the extrusion (`n` times the sign of `d`, `n` the
/// normal of `plane`) somewhere over the profiles' outer loops: the two planes cross over them.
/// The gap between the planes changes linearly across the sketch, so its least value over a
/// loop lies at a line's end or at an arc's end or its point farthest the gap's way down,
/// found exactly; a Bézier or conic piece is sampled to 1e-6 mm. A crossing on a loop's edge,
/// within a hair, does not count.
fn planes_cross_over(from: &Plane, to: &Plane, plane: &Plane, regions: &[Region2], d: f64) -> Result<bool> {
    let gap = |q: Vec2| -> Result<f64> {
        let q = plane.to_world(q);
        Ok((level_on(plane, q, to)? - level_on(plane, q, from)?) * d.signum())
    };
    // The gap's slope across the sketch: it falls fastest along `down`.
    let at0 = gap(Vec2::new(0.0, 0.0))?;
    let slope = Vec2::new(gap(Vec2::new(1.0, 0.0))? - at0, gap(Vec2::new(0.0, 1.0))? - at0);
    let down = (slope * -1.0).normalized();
    let mut points = Vec::new();
    for seg in regions.iter().flat_map(|r| &r.outer.segs) {
        match *seg {
            solidcore_record::Seg2::Line { a, b } => points.extend([a, b]),
            solidcore_record::Seg2::Arc { center, radius, start, sweep } => {
                points.extend([center + Vec2::from_angle(start) * radius, center + Vec2::from_angle(start + sweep) * radius]);
                if let Some(down) = down {
                    // The arc's point at angle `t` is `center + from_angle(t) * radius`, as at its
                    // ends, so with a negative radius its lowest point lies at the angle opposite
                    // `down`. Whether the arc passes that angle, going from `start` by `sweep`:
                    let toward = if radius < 0.0 { down * -1.0 } else { down };
                    let turn = (toward.y.atan2(toward.x) - start) * sweep.signum();
                    if turn.rem_euclid(std::f64::consts::TAU) <= sweep.abs() {
                        points.push(center + toward * radius);
                    }
                }
            }
            _ => points.extend(seg.polyline(1e-6)),
        }
    }
    let mut least = f64::INFINITY;
    let mut reach = 0.0_f64;
    for q in points {
        let g = gap(q)?;
        least = least.min(g);
        reach = reach.max(g.abs());
    }
    Ok(least < -1e-9 * (1.0 + reach))
}

/// Where the line along `plane`'s normal through `centre` meets `face_plane`, as a level along
/// that normal. Refused for a face plane that runs along the normal, which the line never meets.
fn level_on(plane: &Plane, centre: Vec3, face_plane: &Plane) -> Result<f64> {
    let n = plane.normal();
    let m = face_plane.normal();
    let across = n.dot(m);
    if across.abs() < 1e-9 {
        return Err(DocError::Invalid("the From or To face runs along the extrusion".into()));
    }
    Ok(level_along(plane, centre + n * ((face_plane.origin - centre).dot(m) / across)))
}

/// `input`'s part on one side of `plane` ([`kernel::half_space`]), its history carried through
/// the cut: the part's faces of `input` keep their roles, and its face in the plane takes the
/// role `cap`, as a cap of the extrude the part ends.
fn half_space_traced(input: &Traced, plane: &Plane, positive: bool, cap: &str) -> Result<Option<Traced>> {
    let (part, tool) = kernel::half_space(&input.body, plane, positive)?;
    let Some(part) = part else { return Ok(None) };
    // The box's cap in the plane: its start cap on the positive side, its end cap on the other.
    let cap = Some(cap.to_string());
    let caps = if positive { [cap, None] } else { [None, cap] };
    let tool = crate::lineage::ProfileRoles { sketch: None, region: None, caps, cap_synonyms: [None, None], thin: None }.tool(tool, None);
    Ok(Some(Traced::step(part, &[input, &tool])))
}

/// Tools run from `start` to `far` along `plane`'s normal, cut back to `face_plane` (a plane at a
/// slant to the extrusion): each tool's part on the side of `face_plane` its start is on. Refused
/// when a part still has its end cap at `far`, where the plane lies beyond the tool's reach.
fn trim_to_plane(tools: Vec<Traced>, face_plane: &Plane, plane: &Plane, start: f64, far: f64) -> Result<Vec<Traced>> {
    let travel = plane.normal() * (far - start).signum();
    let across = travel.dot(face_plane.normal());
    if across.abs() < 1e-9 {
        return Err(DocError::Invalid("the profile reaches past the To face".into()));
    }
    // Along the travel, the side of the plane before it is the negative one when the travel
    // runs with the plane's normal.
    let positive = across < 0.0;
    let cap = crate::lineage::extrude_cap_role(far);
    let mut out = Vec::new();
    for t in &tools {
        if let Some(part) = half_space_traced(t, face_plane, positive, &cap)? {
            if kernel::has_cap_at(&part.body, plane, far, far - start) {
                return Err(DocError::Invalid("the profile reaches past the To face".into()));
            }
            out.push(part);
        }
    }
    if out.is_empty() {
        return Err(DocError::Invalid("nothing lies between the profile and the To object".into()));
    }
    Ok(out)
}

/// One point on each face of a body (a triangle centre, so it lies on the face).
fn face_samples(b: &ModelBody) -> Vec<Vec3> {
    let m = b.mesh();
    let mut out: Vec<Option<Vec3>> = Vec::new();
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let f = *f as usize;
        if out.len() <= f {
            out.resize(f + 1, None);
        }
        if let (Some(slot @ None), Some([a, bb, c])) = (out.get_mut(f), m.tri(t)) {
            *slot = Some((a + bb + c) * (1.0 / 3.0));
        }
    }
    out.into_iter().flatten().collect()
}

/// The picked faces and every face joined to them smoothly (across tangent edges).
/// Each face appears once, with a point on it. Shell evaluation and selection feedback use
/// the same expansion; callers keep the original picks when saving the feature.
pub fn tangent_face_chain(b: &ModelBody, picks: &[Vec3]) -> Vec<(usize, Vec3)> {
    let m = b.mesh();
    let samples = face_samples(b);
    let face_of = |p: Vec3| crate::appearance::face_index_at(b, p);
    // The surface normal of face f at its mesh vertex nearest q (vertex normals are the
    // surface's, so a tangent edge reads the same from both sides).
    let normal_near = |f: u32, q: Vec3| -> Option<Vec3> {
        m.triangles
            .iter()
            .zip(&m.tri_face)
            .filter(|(_, tf)| **tf == f)
            .flat_map(|(t, _)| t.iter().copied())
            .filter_map(|i| Some((m.positions.get(i as usize)?.dist(q), *m.normals.get(i as usize)?)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .and_then(|(_, n)| n.normalized())
    };
    let mut chosen: Vec<u32> = picks.iter().filter_map(|p| face_of(*p).map(|f| f as u32)).collect();
    chosen.sort_unstable();
    chosen.dedup();
    let mut k = 0;
    while k < chosen.len() && chosen.len() < 10_000 {
        let f = chosen[k];
        k += 1;
        for (e, fs) in m.edges.iter().zip(&m.edge_faces) {
            let (Some(&a), Some(&bb)) = (fs.first(), fs.get(1)) else { continue };
            let other = if a == f {
                bb
            } else if bb == f {
                a
            } else {
                continue;
            };
            if chosen.contains(&other) {
                continue;
            }
            let Some(mid) = e.get(e.len() / 2).copied() else { continue };
            if let (Some(n1), Some(n2)) = (normal_near(f, mid), normal_near(other, mid))
                && n1.dot(n2) > solidcraft_geom::TANGENT_CHAIN.cos()
            {
                chosen.push(other);
            }
        }
    }
    let mut out = Vec::new();
    for f in chosen {
        if let Some(p) = picks.iter().chain(&samples).find(|p| face_of(**p) == Some(f as usize)) {
            out.push((f as usize, *p));
        }
    }
    out
}

fn smooth_chain(b: &ModelBody, picks: &[Vec3]) -> Vec<Vec3> {
    let mut out = picks.to_vec();
    out.extend(tangent_face_chain(b, picks).into_iter().map(|(_, point)| point));
    out
}

/// Why `plane` finds no face when it is attached to a sheet face by the sheet's name for it (see
/// `sheet::plane_sheet_face`, also beneath an offset or a rotation): no sheet names a face so now
/// (see [`crate::sheet::sheet_face_name`]), so the face is gone. A lost attachment is reported,
/// never moved to the nearest face.
pub(crate) fn lost_sheet_face(plane: &PlaneRef) -> DocError {
    let name = crate::sheet::plane_sheet_face(plane).unwrap_or_default();
    DocError::Invalid(format!("lost face attachment: the sheet face `{name}` no longer exists; reattach the sketch to a face"))
}

/// The plane of the body face a sketch sits on, found again: the face its stored `name` finds,
/// which must still be planar and face the picked plane's way (else the sketch fails, asking for
/// its face to be picked again); without a name, among planar faces with the picked plane's
/// normal, the one nearest `at`. The picked frame moves along its normal onto it.
fn face_plane(st: &ModelState, picked: &Plane, at: Vec3, name: Option<&str>) -> Result<(Plane, Option<String>)> {
    let n = picked.normal();
    if let Some(name) = name {
        let hit = crate::naming::locate_face(st, name, at).ok_or_else(|| DocError::reselect(name))?;
        let b = st.bodies.get(hit.body).ok_or_else(|| DocError::reselect(name))?;
        let tol = (b.body.size() * 1e-3).max(1e-3);
        let face = b.body.faces(tol)?.into_iter().find(|f| f.index == hit.face).ok_or_else(|| DocError::reselect(name))?;
        if !face.plane_normal.is_some_and(|fnrm| fnrm.dot(n) > 1.0 - 1e-6) {
            return Err(DocError::reselect(name));
        }
        let note = if !crate::naming::face_name_exact(b, hit.face) && (face.centroid - at).dot(n).abs() > tol {
            // Named from geometry and moved off the picked plane: geometry alone decided.
            Some(format!("face `{}` was found by geometry, not exact history; check it", crate::name_lookup::quoted(name)))
        } else {
            None
        };
        return Ok((picked.offset(face.centroid.dot(n) - picked.origin.dot(n)), note));
    }
    // (distance from `at`, plane offset along n, body, face)
    let mut best: Option<(f64, f64, usize, usize)> = None;
    for (bi, b) in st.bodies.iter().enumerate() {
        let tol = (b.body.size() * 1e-3).max(1e-3);
        let (Ok(faces), Ok(mesh)) = (b.body.faces(tol), b.body.mesh(&solidcraft_kernel::MeshTolerance::chord(tol))) else { continue };
        for f in faces {
            let Some(fnrm) = f.plane_normal else { continue };
            if fnrm.dot(n) < 1.0 - 1e-6 {
                continue;
            }
            let d = mesh
                .triangles
                .iter()
                .zip(&mesh.tri_face)
                .filter(|(_, tf)| **tf as usize == f.index)
                .filter_map(|(t, _)| mesh.tri(t))
                .map(|[a, bb, c]| crate::project::closest_on_triangle(at, a, bb, c).dist(at))
                .fold(f64::INFINITY, f64::min);
            if best.is_none_or(|x| d < x.0) {
                best = Some((d, f.centroid.dot(n), bi, f.index));
            }
        }
    }
    Ok(match best {
        Some((_, off, bi, face)) => {
            let shift = off - picked.origin.dot(n);
            (picked.offset(shift), off_its_face(st, bi, face, at, n, off))
        }
        None => (*picked, Some("the face the sketch was on is gone; the sketch stays where it was".into())),
    })
}

/// The note for a sketch whose unnamed face point `at` is on no face now: `at`, moved along `n`
/// onto the plane (offset `off`) of face `face` of body `bi`, the face nearest it, is decided
/// exactly on that face (#20518). A face that moved along its normal (a taller box) still holds
/// the point and says nothing; a pick off every face (the `sketch.create` command refuses it)
/// keeps the sketch on that face's plane, but says so.
fn off_its_face(st: &ModelState, bi: usize, face: usize, at: Vec3, n: Vec3, off: f64) -> Option<String> {
    let b = st.bodies.get(bi).filter(|b| !b.body.is_mesh())?;
    let tol = (b.body.size() * 1e-3).max(1e-3);
    let on_plane = at + n * (off - at.dot(n));
    match b.body.point_lies_on_face(face, on_plane, tol) {
        Ok(None) => Some(format!(
            "no face is at the point the sketch was placed on, [{}, {}, {}]; the sketch is on the plane of the nearest flat face \
             facing the same way: check it",
            at.x, at.y, at.z
        )),
        _ => None,
    }
}

/// A chamfer, equal or by its second distance or angle.
fn chamfer_with(
    vals: &BTreeMap<String, Value>,
    b: &kernel::Body,
    edges: &[Vec3],
    d: f64,
    distance2: &Option<crate::Expr>,
    angle: &Option<crate::Expr>,
    flip: bool,
) -> Result<kernel::Body> {
    let side = match (distance2, angle) {
        (Some(e), _) => Some(kernel::ChamferSide::Distance(val(vals, e, Kind::Length)?)),
        (None, Some(e)) => Some(kernel::ChamferSide::Angle(val(vals, e, Kind::Angle)?)),
        (None, None) => None,
    };
    Ok(match side {
        Some(s) => match kernel::chamfer_sides_carrying_face(b, edges, d, s, flip) {
            // SMLib can fail a two-sided chamfer too large for the faces beside its edges inside
            // the blend (status 1001) instead of refusing it as too large (#18545): where a
            // smaller scale of its sizes builds, it is refused naming that scale.
            Err(kernel::KernelError::Failed(why)) if why.contains("(status 1001)") && EVAL_VALIDATION.get() != kernel::Validation::Off => {
                match kernel::chamfer_sides_max_why(b, edges, d, s, flip) {
                    Ok(kernel::FitSearch { found: Some((scale, _)), .. }) => {
                        // By an angle only the distance scales: it is named in mm.
                        let fits = match s {
                            kernel::ChamferSide::Distance(_) => kernel::scale_fit_note(scale),
                            kernel::ChamferSide::Angle(_) => kernel::largest_fit_note("distance", d * scale),
                        };
                        return Err(
                            kernel::KernelError::TooLarge(format!("the chamfer is larger than the faces beside its edges allow; {fits}")).into()
                        );
                    }
                    _ => return Err(kernel::KernelError::Failed(why).into()),
                }
            }
            // A refusal as too large that names no fit: as for an equal chamfer, it names the
            // largest scale of its sizes that builds, or says that none tried builds (#19691).
            Err(kernel::KernelError::TooLarge(why)) if !kernel::names_largest_fit(&why) && EVAL_VALIDATION.get() != kernel::Validation::Off => {
                let why = match kernel::chamfer_sides_max_why(b, edges, d, s, flip) {
                    Ok(kernel::FitSearch { found: Some((scale, _)), .. }) => match s {
                        kernel::ChamferSide::Distance(_) => format!("{why}; {}", kernel::scale_fit_note(scale)),
                        kernel::ChamferSide::Angle(_) => format!("{why}; {}", kernel::largest_fit_note("distance", d * scale)),
                    },
                    Ok(kernel::FitSearch { found: None, smallest_refused: Some((at, e)), tried, .. }) => format!(
                        "{why}; none of the {tried} scales of its sizes tried builds (the smallest, {} times: {})",
                        show_mm(at),
                        kernel::without_fit_note(&e.to_string())
                    ),
                    _ => why,
                };
                return Err(kernel::KernelError::TooLarge(why).into());
            }
            other => other?,
        },
        None => match kernel::chamfer_carrying_face(b, edges, d) {
            Err(kernel::KernelError::TooLarge(why)) => {
                let hint = kernel::named_fit(&why);
                return Err(with_largest_fit(why, "distance", || kernel::chamfer_max_why(b, edges, d, hint)).into());
            }
            other => other?,
        },
    })
}

/// Evaluate one feature into its scratch state; the caller commits it only after validation.
#[allow(clippy::too_many_arguments)]
fn eval_feature(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &mut ModelState,
    warning: &mut Option<String>,
    import_report: &mut Option<Arc<kernel::ImportReport>>,
    repair: &mut Option<Arc<kernel::RepairReport>>,
    heal: bool,
    prepared_meshes: Option<&PreparedMeshImport>,
) -> Result<()> {
    JOINED_TARGETS.with(|n| n.borrow_mut().clear());
    let r = eval_feature_kind(doc, vals, f, st, warning, import_report, repair, heal, prepared_meshes);
    for note in JOINED_TARGETS.with(|n| std::mem::take(&mut *n.borrow_mut())) {
        append_warning(warning, note);
    }
    r
}

/// The start and end of the note a feature built from a sketch with a lost link carries.
const RETAINED_NOTE_START: &str = "built from Sketch ";
const RETAINED_NOTE_END: &str = "'s last outline: its projected geometry could not be updated; pick the face again in the sketch";

/// A note for each sketch the feature is built from whose linked outline could not be refreshed
/// (a link marked lost keeps its last geometry): the feature then follows that old outline, and
/// says so on its own row, not only on the sketch's. `st` is the state after the feature.
fn retained_outline_notes(doc: &Document, f: &Feature, st: &ModelState) -> Vec<String> {
    let mut ids = f.kind.input_sketches();
    ids.sort_unstable();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| st.sketch(id))
        .filter(|ss| ss.sketch.links.iter().any(|l| l.lost))
        // The sketch's name in the design now: a state restored from before a rename holds the old one.
        .map(|ss| format!("{RETAINED_NOTE_START}{}{RETAINED_NOTE_END}", doc.feature(ss.feature).map_or(ss.name.as_str(), |k| k.name.as_str())))
        .collect()
}

/// `warning` without the retained-outline notes it holds. They are never kept in a cached result,
/// since they follow the sketches' names and links as they are now.
pub(crate) fn without_retained_notes(warning: Option<String>) -> Option<String> {
    let mut s = warning?;
    while let Some(a) = s.match_indices(RETAINED_NOTE_START).map(|(i, _)| i).find(|&i| i == 0 || s[..i].ends_with("; ")) {
        let Some(rel) = s[a..].find(RETAINED_NOTE_END) else { break };
        let b = a + rel + RETAINED_NOTE_END.len();
        let (from, to) = if a >= 2 {
            (a - 2, b)
        } else if s[b..].starts_with("; ") {
            (a, b + 2)
        } else {
            (a, b)
        };
        s.replace_range(from..to, "");
    }
    (!s.is_empty()).then_some(s)
}

/// `warning` with the retained-outline notes the feature has now (replacing any it held), read
/// from `st`, the state after it: a saved model or a rename cannot leave an old one.
pub(crate) fn with_retained_notes(doc: &Document, warning: Option<String>, f: &Feature, st: &ModelState) -> Option<String> {
    let mut warning = without_retained_notes(warning);
    for note in retained_outline_notes(doc, f, st) {
        append_warning(&mut warning, note);
    }
    warning
}

/// [`eval_feature`] for one kind of feature. `heal` is the tolerance heal preference the
/// evaluation read once ([`Model::heals`]): Repair heals first as it says, whatever the
/// preference became since.
#[allow(clippy::too_many_arguments)]
fn eval_feature_kind(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &mut ModelState,
    warning: &mut Option<String>,
    import_report: &mut Option<Arc<kernel::ImportReport>>,
    repair: &mut Option<Arc<kernel::RepairReport>>,
    heal: bool,
    prepared_meshes: Option<&PreparedMeshImport>,
) -> Result<()> {
    // Its tools meet bodies in other placements where those are shown (#3383).
    let _frames = ToolFrames::install(doc, f, st);
    refuse_ambiguous_targets(doc, st, f, &f.participants)?;
    match &f.kind {
        FeatureKind::NewerFileFeature { raw } => Err(DocError::Invalid(raw.why())),
        FeatureKind::BodyTransfer { bodies, from, to, transform } => {
            crate::body_transfer::evaluate(doc, st, &crate::plain(bodies), *from, *to, *transform)
        }
        FeatureKind::SheetBase { .. }
        | FeatureKind::SheetContour { .. }
        | FeatureKind::SheetFlange { .. }
        | FeatureKind::SheetHem { .. }
        | FeatureKind::SheetPerforate { .. }
        | FeatureKind::SheetFold { .. }
        | FeatureKind::SheetPath { .. }
        | FeatureKind::SheetTab { .. }
        | FeatureKind::SheetUnfold { .. }
        | FeatureKind::SheetConvert { .. }
        | FeatureKind::SheetSurfaceConvert { .. }
        | FeatureKind::SheetRecognize { .. } => more::sheet_eval(doc, vals, f, st, warning),
        FeatureKind::Boss { .. } | FeatureKind::Lip { .. } | FeatureKind::SnapFit { .. } | FeatureKind::Rest { .. } => {
            more::plastic_eval(doc, vals, f, st)
        }
        FeatureKind::Patch { .. }
        | FeatureKind::Stitch { .. }
        | FeatureKind::Thicken { .. }
        | FeatureKind::SurfaceTrim { .. }
        | FeatureKind::SurfaceExtend { .. }
        | FeatureKind::SurfaceExtrude { .. }
        | FeatureKind::Unstitch { .. }
        | FeatureKind::ReverseNormal { .. }
        | FeatureKind::SurfaceRevolve { .. }
        | FeatureKind::SurfaceSweep { .. }
        | FeatureKind::SurfaceLoft { .. }
        | FeatureKind::SurfaceBlend { .. }
        | FeatureKind::SurfaceBoundary { .. }
        | FeatureKind::SurfaceNetwork { .. }
        | FeatureKind::SurfaceMatch { .. }
        | FeatureKind::SurfaceFair { .. }
        | FeatureKind::SurfaceOffset { .. }
        | FeatureKind::SurfaceRuled { .. }
        | FeatureKind::SurfaceUntrim { .. } => surface::eval(doc, vals, f, st, warning),
        FeatureKind::Emboss { .. }
        | FeatureKind::Rib { .. }
        | FeatureKind::ReplaceFace { .. }
        | FeatureKind::Align { .. }
        | FeatureKind::Remove { .. } => more::eval(doc, vals, f, st, warning),
        FeatureKind::Sketch { plane, sketch } => eval_sketch(doc, vals, f, plane, sketch, st, warning, crate::project::LostNames::Fail),
        FeatureKind::Extrude { operation, targets, .. }
        | FeatureKind::Pipe { operation, targets, .. }
        | FeatureKind::Revolve { operation, targets, .. }
        | FeatureKind::Loft { operation, targets, .. }
        | FeatureKind::Coil { operation, targets, .. }
        | FeatureKind::Sweep { operation, targets, .. } => {
            refuse_ambiguous_targets(doc, st, f, &crate::plain(targets))?;
            let mut constructor = None;
            let mut to_body = None;
            let tools = feature_tools_with_constructor(doc, vals, f, st, &mut constructor, &mut to_body, warning)?;
            // #9969: a cut whose tool lies clear of the body (a profile drawn beside it, a
            // distance that stops short, the wrong direction) reported success and changed
            // nothing. Say so, as a hole does (#10701): the bodies are compared before and after,
            // whether the cut went through sheet metal's flat pattern or the generic path. A body
            // the cut rebuilt with the volume it had lost nothing either: a tool lying in an
            // empty slot, touching its walls, splits their faces and removes nothing (#18825).
            // A cut that removes material usually leaves a face on a surface the body didn't
            // have (a wall of the pocket). Measuring volumes takes far longer than the cut, so
            // they are only compared when the body shows no such face.
            let before: Option<Vec<Body>> = (*operation == Operation::Cut).then(|| st.bodies.iter().map(|b| b.body.clone()).collect());
            let unchanged = |st: &ModelState, warning: &mut Option<String>| {
                // Equal to rounding only: a body the cut rebuilt without removing anything has the
                // same exact volume (the faces it split add up to the faces they were), while a cut
                // can remove very little of a large body (a 0.005 mm wall between two pockets in a
                // 1 m cube is 5e-10 of it).
                let same_volume = |old: &Body, new: &Body| match (old.volume_at(0.0), new.volume_at(0.0)) {
                    (Some(a), Some(b)) => (a - b).abs() <= 1e-12 * a.abs().max(b.abs()),
                    _ => false,
                };
                let kept = |b: &ModelBody, old: &Body| {
                    old.same(&b.body) || (!b.body.is_mesh() && !old.is_mesh() && !has_a_new_surface(old, &b.body) && same_volume(old, &b.body))
                };
                if let Some(before) = &before
                    && !before.is_empty()
                    && st.bodies.len() == before.len()
                    && st.bodies.iter().zip(before).all(|(b, old)| kept(b, old))
                {
                    append_warning(warning, "the cut removes no material; check where its profile lies, the direction and the distance".into());
                }
            };
            // An extrude that cuts a sheet made from surfaces leaves pockets as deep as it was drawn,
            // and none of a layer thinner than the extrusion resolution.
            if matches!(operation, Operation::Cut | Operation::Intersect)
                && matches!(f.kind, FeatureKind::Extrude { .. })
                && !st.surface_sheets.is_empty()
                && let Some(c) = &constructor
            {
                let reach: Vec<_> = tools.iter().map(|t| more::exact_box(&t.body)).collect();
                more::surface_thin_layer_refused(st, &reach, &c.plane, c.interval, &crate::plain(targets))?;
                if *operation == Operation::Cut {
                    more::surface_corner_graze_refused(f, st, &crate::plain(targets), c)?;
                }
                st.cut_extents.insert(f.id, (c.plane, c.interval));
            }
            // A tapered cut's walls are not square to a sheet made from surfaces: refused where it
            // stops inside a flat panel's thickness, and an intersect where it changes one. A
            // tapered cut through the sheet goes on to be read as its shadow.
            if matches!(operation, Operation::Cut | Operation::Intersect)
                && matches!(&f.kind, FeatureKind::Extrude { extent, .. } if extent.taper.is_some())
                && !st.surface_sheets.is_empty()
            {
                let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
                more::surface_taper_refused(st, &bodies, constructor.as_ref(), &crate::plain(targets), *operation)?;
            }
            // A cut square through flat panels of sheets made from surfaces goes into their outlines.
            if *operation == Operation::Cut && matches!(f.kind, FeatureKind::Extrude { .. }) && !st.surface_sheets.is_empty() {
                let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
                let handled = more::surface_prism_cut(f, st, &bodies, &crate::plain(targets), constructor.as_ref())?;
                if !handled.is_empty() {
                    apply_beside_sheet_cut(doc, st, f, tools, *operation, &crate::plain(targets), &handled)?;
                    unchanged(st, warning);
                    return Ok(());
                }
            }
            // A join one sheet thick in the plane of a flat panel of a sheet made from surfaces
            // becomes part of that panel. A Join to an object (#6424) is left to the generic path
            // below, which joins the object as well.
            if *operation == Operation::Join
                && to_body.is_none()
                && matches!(f.kind, FeatureKind::Extrude { .. })
                && !st.surface_sheets.is_empty()
                && !more::surface_sheet_join(f, st, &crate::plain(targets), constructor.as_ref())?.is_empty()
            {
                return Ok(());
            }
            // A Join extrude to an object joins that object too (#6424): the tool and the body
            // it runs to are one body, not only the body the profile was sketched on.
            let extra: Vec<String> = if *operation == Operation::Join { to_body.into_iter().collect() } else { Vec::new() };
            apply_traced_with(doc, st, f, tools, *operation, &crate::plain(targets), &extra)?;
            unchanged(st, warning);
            Ok(())
        }
        FeatureKind::CellGrid { targets, mode, .. } => {
            let tools = cell_grid::tools(doc, vals, f, st, warning)?;
            // A grid beside the body, or cut the wrong way, changes nothing: say so, as a cut does.
            // Raised cells are joined, as a join extrude's tool is: with no body to join they are
            // a new body, and one that stands inside the body changes nothing, with no warning.
            let before: Vec<Body> = st.bodies.iter().map(|b| b.body.clone()).collect();
            let op = match mode {
                crate::CellGridMode::Cut => Operation::Cut,
                crate::CellGridMode::Raised => Operation::Join,
            };
            apply_traced(doc, st, f, tools, op, &crate::plain(targets))?;
            if op == Operation::Cut
                && !before.is_empty()
                && st.bodies.len() == before.len()
                && st.bodies.iter().zip(&before).all(|(b, old)| old.same(&b.body))
            {
                append_warning(warning, "the cut removes no material; check where its profile lies, the direction and the depth".into());
            }
            Ok(())
        }
        FeatureKind::Hole { thread, thread_depth, thread_class, modeled, ease_out, depth, to, .. } => {
            if st.bodies.is_empty() {
                return Err(empty_boolean_targets(doc, st, f, &[], f.participants.is_empty()));
            }
            let thread_depth = thread_depth.as_ref().map(|d| val(vals, d, Kind::Length)).transpose()?;
            if thread_depth.is_some() && thread.is_none() {
                return Err(DocError::Invalid("thread depth requires a tapped hole".into()));
            }
            // A class the command would refuse (a hand-edited file) fails the hole, saying why.
            // An unknown thread fails where the thread is cut, with its own reason.
            match (thread_class, thread) {
                (None, _) => {}
                (Some(_), None) => return Err(DocError::Invalid("a thread class needs a tapped hole".into())),
                (Some(class), Some(t)) => {
                    if let Some(tapped) = crate::parse_tapped_thread(t) {
                        tapped.check_class(class).map_err(DocError::Invalid)?;
                    }
                }
            }
            if *modeled && thread.is_none() {
                return Err(DocError::Invalid("a modelled thread needs a tapped hole".into()));
            }
            if ease_out.is_some() && !*modeled {
                return Err(DocError::Invalid("a runout is cut only on a modelled thread".into()));
            }
            if thread_depth.is_some_and(|d| !d.is_finite() || d <= 0.0) {
                return Err(DocError::Invalid("thread depth must be finite and positive".into()));
            }
            let (spots, dir) = hole_spots(doc, vals, st, f)?;
            let points_gone = hole_points_gone(st, f);
            if !points_gone.is_empty() {
                append_warning(
                    warning,
                    format!("sketch point(s) {} no longer in the sketch: not drilled there (edit the Hole to drop them)", backticked(&points_gone)),
                );
            }
            // How deep each hole goes (through all: as far as the wall goes), read before the cut:
            // the pieces of a wall the thread runs along lie within it.
            let reaches: Vec<f64> = if thread.is_some() {
                spots
                    .iter()
                    .map(|spot| match hole_depth_to(st, &spot.at, dir, &to.map(Vec3::from), depth)? {
                        Some(e) => val(vals, &e, Kind::Length),
                        None => Ok(f64::INFINITY),
                    })
                    .collect::<Result<_>>()?
            } else {
                Vec::new()
            };
            let tools = hole_feature_tools(doc, vals, st, f)?;
            // A drill square through a flat panel of a sheet made from surfaces the hole cuts goes
            // into its outline, and one along the panel into its rim is a bore of it.
            let handled = if st.surface_sheets.is_empty() { Vec::new() } else { more::surface_hole_cut(f, st, &tools)? };
            let tools: Vec<Traced> = tools.into_iter().map(|tool| tool.traced).collect();
            if handled.is_empty() {
                // A hole moved off the material (a changed position, parameter or sketch point)
                // cuts nothing: it says so, as a pattern does of its copies (#10701).
                let missed = tools.iter().filter(|tool| hole_misses(st, f, &tool.body)).count();
                let total = tools.len();
                let targets: Vec<String> = hole_targets(st, f).iter().map(|b| b.name.clone()).collect();
                apply_traced(doc, st, f, tools, Operation::Cut, &[])?;
                match (missed, total) {
                    (0, _) => {}
                    (_, 1) => append_warning(warning, "the hole removes no material".into()),
                    _ => append_warning(warning, format!("{missed} of {total} holes remove no material")),
                }
                // A hole wider than the part takes all of it away (#10702): almost never what a
                // hole means, so it says which bodies are gone.
                let gone: Vec<&str> = targets.iter().map(String::as_str).filter(|n| st.body(n).is_none()).collect();
                if !gone.is_empty() {
                    append_warning(warning, format!("the hole removes all of {}", gone.join(", ")));
                }
            } else {
                apply_beside_sheet_cut(doc, st, f, tools, Operation::Cut, &[], &handled)?;
            }
            hole_threads(vals, st, f, f.id, spots.iter().map(|spot| spot.at), dir, &reaches, false)?;
            Ok(())
        }
        FeatureKind::Thread { .. } => thread::eval(doc, vals, f, st, warning),
        FeatureKind::Shell { faces, thickness, body, direction, tangent_chain } => {
            let t = val(vals, thickness, Kind::Length)?;
            // No face opened (a closed hollow body) picks no body: the one there is, or the named one.
            let i = match (faces.is_empty(), body, st.bodies.len()) {
                (true, None, 1) => 0,
                (true, None, _) => return Err(DocError::Invalid("a shell that opens no face needs the body named".into())),
                _ => body_at(st, body, &crate::plain(faces))?,
            };
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let picks = crate::plain(faces);
            let face_count = mb.body.face_count();
            if face_count > 0 && picks.len() >= face_count {
                // Match the shell's picking reach, using exact faces. Every authored pick must
                // resolve; leave a bad pick to the normal selection error. Repeated picks count once.
                let reach = mb.body.size() * 1e-3 + 1e-6;
                let picked: Option<Vec<_>> = picks.iter().map(|p| mb.body.nearest_face(*p).filter(|(d, _)| *d < reach).map(|(_, f)| f)).collect();
                if let Some(mut picked) = picked {
                    picked.sort_unstable();
                    picked.dedup();
                    if picked.len() == face_count {
                        return Err(DocError::Invalid(
                            "Removing every face leaves no walls. Select the body without faces for a closed hollow body, or select fewer faces."
                                .into(),
                        ));
                    }
                }
            }
            let open = if *tangent_chain {
                let chain = tangent_face_chain(&mb, &picks);
                if !chain.is_empty() && chain.len() == mb.body.face_count() {
                    return Err(DocError::Invalid(
                        "Tangent Chain opens every face of this body, leaving no walls; turn off Tangent Chain to remove only the picked faces"
                            .into(),
                    ));
                }
                // Keep every authored pick, including any the kernel must reject, as before
                // chain expansion. The derived samples must not silently drop a bad input.
                let mut open = picks;
                open.extend(chain.into_iter().map(|(_, point)| point));
                open
            } else {
                picks
            };
            let input = mb.traced();
            // The shell, and the operands of the operation that made it.
            let (nb, operands) = match direction.as_str() {
                "" | "inside" => (kernel::shell(&mb.body, &open, t)?, vec![input]),
                "outside" | "both" => {
                    // Walls grown out of the faces (or half out, half in): an offset body less a
                    // cavity, the open faces pushed out of the way.
                    if !(t > 1e-6) {
                        return Err(DocError::Invalid("shell thickness must be positive".into()));
                    }
                    let (out, inn) = if direction == "outside" { (t, 0.0) } else { (t / 2.0, t / 2.0) };
                    let samples = face_samples(&mb);
                    let opened =
                        |p: &Vec3| open.iter().any(|q| crate::appearance::face_index_at(&mb, *q) == crate::appearance::face_index_at(&mb, *p));
                    let walls: Vec<Vec3> = samples.iter().filter(|p| !opened(p)).copied().collect();
                    let outer = Traced::step(kernel::offset_faces(&mb.body, &walls, out)?, &[&input]);
                    // The cavity's faces are the shell's inner walls, named after the faces they offset.
                    let inner = mb.before_as(crate::lineage::Operand::Inner(mb.name.clone()));
                    let cavity = if inn > 0.0 { Traced::step(kernel::offset_faces(&mb.body, &walls, -inn)?, &[&inner]) } else { inner };
                    // The open faces pushed out of the way (none: the cavity stays sealed inside).
                    let cavity =
                        if open.is_empty() { cavity } else { Traced::step(kernel::offset_faces(&cavity.body, &open, 2.0 * t + 1.0)?, &[&cavity]) };
                    let cut = kernel::boolean(&outer.body, &cavity.body, BoolOp::Cut)?
                        .ok_or_else(|| DocError::Invalid("the shell removed everything".into()))?;
                    (cut, vec![outer, cavity])
                }
                o => return Err(DocError::Invalid(format!("unknown shell direction `{o}` (inside, outside or both)"))),
            };
            // Certifying may join faces; their history stays over the same operands.
            let nb = Traced::step(nb.with_shell_certificate(f.id)?, &operands.iter().collect::<Vec<_>>());
            // The walls must read back as SMLib reads them for measuring and every step after
            // (#18373: a 0.06 mm hollow of a tiny lens left a face whose NURBS form isn't its
            // surface). Refused here, the body stays as it was. The reading is kept on the body.
            if !nb.body.is_mesh()
                && let Err(e) = nb.body.smlib()
            {
                return Err(DocError::Invalid(format!("the shell made walls that can't be used afterwards ({e})")));
            }
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = mb.remade(nb);
            }
            Ok(())
        }
        FeatureKind::Draft { faces, angle, neutral, pull, body } => {
            let a = val(vals, angle, Kind::Angle)?;
            let pl = doc.resolve_plane_in(vals, neutral, 0, f.component, Some(st))?;
            let i = body_at(st, body, &crate::plain(faces))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let nb = Traced::step(kernel::draft(&mb.body, &crate::plain(faces), &pl, pull.0, a)?, &[&mb.traced()]);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = mb.remade(nb);
            }
            Ok(())
        }
        FeatureKind::ConstructionPlane { plane } => {
            // On a body face: follow the face as it is now, as a sketch on that face does.
            let pl = match crate::face_anchor::plane_face_anchor(plane) {
                Some(anchor) => {
                    let fst = crate::state_in_face_anchor_frame(doc, st, anchor, f.component)?;
                    let (pl, note) = doc.resolve_attached_plane_noted(vals, plane, f.component, st, &fst)?;
                    if let Some(note) = note {
                        append_warning(warning, note);
                    }
                    pl
                }
                // On a flat face of a sheet by the sheet's name for it: no such face now.
                None if crate::sheet::plane_sheet_face(plane).is_some() => return Err(lost_sheet_face(plane)),
                // A plane of numbers alone reads no body: placing them all is the design's size.
                None if plane_is_fixed(plane) => doc.resolve_plane_in(vals, plane, 0, f.component, Some(st))?,
                // Picks on bodies of other (moved) components are where those bodies are shown.
                None => doc.resolve_plane_in(
                    vals,
                    &*construction_planes_in_frame(doc, st, f.component, plane)?,
                    0,
                    f.component,
                    Some(&*construction_state(doc, st, f.component)?),
                )?,
            };
            st.construct.push(crate::construct::Construct {
                feature: f.id,
                name: f.name.clone(),
                geom: crate::construct::ConstructGeom::Plane(pl),
                bodies: Vec::new(),
            });
            Ok(())
        }
        FeatureKind::ConstructionAxis { def } => {
            let cst = construction_state(doc, st, f.component)?;
            let ctx = crate::construct::Ctx { doc, vals, st: &cst, depth: 0, component: f.component };
            let (origin, dir) = crate::construct::resolve_axis(&ctx, def)?;
            let bodies = crate::construct::source_bodies(&ctx, &def.refs());
            st.construct.push(crate::construct::Construct {
                feature: f.id,
                name: f.name.clone(),
                geom: crate::construct::ConstructGeom::Axis { origin, dir },
                bodies,
            });
            Ok(())
        }
        FeatureKind::ConstructionPoint { def } => {
            let cst = construction_state(doc, st, f.component)?;
            let ctx = crate::construct::Ctx { doc, vals, st: &cst, depth: 0, component: f.component };
            let p = crate::construct::resolve_point(&ctx, def)?;
            let bodies = crate::construct::source_bodies(&ctx, &def.refs());
            st.construct.push(crate::construct::Construct {
                feature: f.id,
                name: f.name.clone(),
                geom: crate::construct::ConstructGeom::Point(p),
                bodies,
            });
            Ok(())
        }
        FeatureKind::BoundaryFill { tools, cells, operation, remove_tools } => {
            if tools.is_empty() || tools.len() > 64 || cells.is_empty() || cells.len() > 1000 {
                return Err(DocError::Invalid("Boundary Fill needs 1…64 tool bodies and 1…1000 cells".into()));
            }
            let tb: Vec<ModelBody> =
                tools.iter().map(|t| st.body(t).cloned().ok_or_else(|| DocError::Unknown(format!("body `{t}`")))).collect::<Result<_>>()?;
            // This construction combines solid interiors; open sheets do not define one.
            if tb.iter().any(|b| b.body.is_mesh() || b.body.is_surface()) {
                return Err(DocError::Invalid("Boundary Fill needs closed solid tools; open surface and mesh tools are not supported".into()));
            }
            let mut made = Vec::new();
            for p in cells {
                // The cell: inside the tools that hold the point, outside the others.
                let inside: Vec<bool> = tb
                    .iter()
                    .map(|b| match b.body.classify_point(p.0)? {
                        kernel::PointClass::Inside => Ok(true),
                        kernel::PointClass::Outside => Ok(false),
                        kernel::PointClass::OnBoundary => {
                            Err(DocError::Invalid("Boundary Fill cell points must be away from tool boundaries".into()))
                        }
                    })
                    .collect::<Result<_>>()?;
                let mut acc: Option<Traced> = None;
                for (b, _) in tb.iter().zip(&inside).filter(|(_, i)| **i) {
                    acc = match acc {
                        None => Some(b.traced()),
                        Some(a) => kernel::boolean(&a.body, &b.body, BoolOp::Intersect)?.map(|r| Traced::step(r, &[&a, &b.traced()])),
                    };
                }
                let Some(mut cell) = acc else {
                    return Err(DocError::Invalid(format!("the point {} is outside every tool: no enclosed cell there", p.0)));
                };
                for (b, _) in tb.iter().zip(&inside).filter(|(_, i)| !**i) {
                    let cut = kernel::boolean(&cell.body, &b.body, BoolOp::Cut)?.ok_or_else(|| DocError::Invalid("a cell came out empty".into()))?;
                    cell = Traced::step(cut, &[&cell, &b.traced()]);
                }
                if cell.body.is_surface() {
                    return Err(DocError::Invalid("Boundary Fill did not produce a closed solid cell".into()));
                }
                made.push(cell);
            }
            if *remove_tools {
                st.bodies.retain(|b| !tools.iter().any(|t| *t == b.name));
            }
            apply_traced(doc, st, f, made, *operation, &[])
        }
        FeatureKind::SplitFace { faces, body, plane, tool, sketch, loops } => {
            let i = body_at(st, body, &crate::plain(faces))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let mut picked: Vec<usize> = Vec::new();
            for p in faces {
                let fi = crate::appearance::face_index_at(&mb, p.0)
                    .ok_or_else(|| DocError::Invalid(format!("Split Face: no face of {} at {}", mb.name, p.0)))?;
                if !picked.contains(&fi) {
                    picked.push(fi);
                }
            }
            let nb = match (tool, sketch) {
                (Some(t), _) => {
                    let tb = st.body(&t.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", t.body)))?;
                    let face = crate::appearance::face_index_at(tb, t.point.0)
                        .ok_or_else(|| DocError::Invalid(format!("no face of `{}` at the tool point", t.body)))?;
                    kernel::split_faces(&mb.body, &picked, &kernel::SplitTool::Face { body: &tb.body, face })?
                }
                (None, Some(sid)) => {
                    let ss = st.sketch(sid.0).ok_or_else(|| DocError::Unknown(format!("sketch {sid}")))?;
                    // Each curve's exact segments (lines and arcs split by their own plane and cylinder),
                    // or with `loops` the curves joined end to end: a closed chain is not extended.
                    let curves: Vec<Vec<solidcore_record::Seg2>> = if *loops {
                        let ids: Vec<String> = ss.sketch.curves.iter().filter(|c| !c.construction).map(|c| c.id.clone()).collect();
                        surface::curve_chains(ss, &ids)?
                    } else {
                        (0..ss.sketch.curves.len())
                            .filter(|ci| ss.sketch.curves.get(*ci).is_some_and(|c| !c.construction))
                            .map(|ci| ss.sketch.segs(ci))
                            .filter(|segs| !segs.is_empty())
                            .collect()
                    };
                    if curves.is_empty() {
                        return Err(DocError::Invalid(format!("sketch `{}` has no curves to split with", ss.name)));
                    }
                    kernel::split_faces(&mb.body, &picked, &kernel::SplitTool::Curves { plane: ss.plane, curves, along: None })?
                }
                (None, None) => {
                    let pl = doc.resolve_plane_in(vals, plane, 0, f.component, Some(st))?;
                    kernel::split_faces(&mb.body, &picked, &kernel::SplitTool::Plane(pl))?
                }
            };
            let nb = Traced::step(nb, &[&mb.traced()]);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = mb.remade(nb);
            }
            Ok(())
        }
        FeatureKind::Split { body, plane, tool, tool_plane, tool_sheet, tool_faces, sketch, line } => {
            let i = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Unknown(format!("body `{body}`"))) };
            let input = mb.traced();
            let parts = match tool {
                // A sketch line: the plane through it along the sketch's normal.
                _ if sketch.is_some() || line.is_some() => {
                    let (Some(sketch), Some(line)) = (sketch, line) else {
                        return Err(DocError::Invalid("a sketch line tool needs both its sketch and its line".into()));
                    };
                    split_by_plane_traced(&input, &sketch_line_plane(st, sketch.0, line)?)?
                }
                // A face of a body (its surface extended).
                Some(t) => {
                    let tb = st.body(&t.body).ok_or_else(|| DocError::Unknown(format!("body `{}`", t.body)))?;
                    let face = crate::appearance::face_index_at(tb, t.point.0)
                        .ok_or_else(|| DocError::Invalid(format!("no face of `{}` at the tool point", t.body)))?;
                    if *tool_plane {
                        let plane =
                            tb.body.planar_face(face).ok_or_else(|| DocError::Invalid("the splitting tool face is no longer planar".into()))?;
                        let source = body_world(doc, tb);
                        let target = body_world(doc, &mb);
                        let plane = if source == target {
                            plane
                        } else {
                            let inverse = crate::mat_inverse(&target)
                                .ok_or_else(|| DocError::Invalid(format!("the component holding `{body}` has a singular placement")))?;
                            placed_plane(&crate::mat_mul(&inverse, &source), &plane)?
                        };
                        split_by_plane_traced(&input, &plane)?
                    } else {
                        // Faces used as they are cut where they are shown: a tool placed
                        // differently from the body is moved into the body's frame (#9305).
                        // Moving keeps the face numbers, so `face` still names the same face.
                        let sheet = |tb: &ModelBody| -> Result<kernel::Body> {
                            let source = body_world(doc, tb);
                            let target = body_world(doc, &mb);
                            if source == target {
                                return Ok(tb.body.clone());
                            }
                            let inverse = crate::mat_inverse(&target)
                                .ok_or_else(|| DocError::Invalid(format!("the component holding `{body}` has a singular placement")))?;
                            Ok(kernel::transform_matrix(&tb.body, crate::mat_mul(&inverse, &source))?)
                        };
                        let parts = match tool_sheet {
                            crate::SplitSheet::Extended => kernel::split_body(&mb.body, &kernel::SplitTool::Face { body: &tb.body, face })?,
                            // The picked faces as they are, never extended (#9305).
                            crate::SplitSheet::Faces => {
                                let faces = tool_faces
                                    .iter()
                                    .map(|p| {
                                        crate::appearance::face_index_at(tb, p.0)
                                            .ok_or_else(|| DocError::Invalid(format!("no face of `{}` at a tool point", t.body)))
                                    })
                                    .collect::<Result<Vec<_>>>()?;
                                kernel::split_body_by_faces(&mb.body, &sheet(tb)?, face, &faces)?
                            }
                            // Every face of a surface body as it is (#9305).
                            crate::SplitSheet::Body => {
                                if !tb.body.is_surface() {
                                    return Err(DocError::Invalid(format!("`{}` is not a surface body, so it cannot split as a whole", t.body)));
                                }
                                let faces: Vec<usize> = (0..tb.body.face_count()).collect();
                                kernel::split_body_by_faces(&mb.body, &sheet(tb)?, face, &faces)?
                            }
                        };
                        // The pieces keep the planar roles of the faces they share with the body (#565).
                        parts
                            .into_iter()
                            .map(|part| Ok(Traced::step(part.with_preserved_planar_sources(&mb.body)?, &[&input])))
                            .collect::<Result<Vec<_>>>()?
                    }
                }
                None => {
                    let pl = doc.resolve_plane_in(vals, plane, 0, f.component, Some(st))?;
                    split_by_plane_traced(&input, &pl)?
                }
            };
            if parts.len() < 2 {
                return Err(DocError::Invalid("the tool does not split the body".into()));
            }
            let parts = split_side_lumps(&mb.body, parts);
            let mut it = parts.into_iter();
            if let (Some(first), Some(slot)) = (it.next(), st.bodies.get_mut(i)) {
                *slot = mb.remade(first);
            }
            for (k, b) in it.enumerate() {
                // A piece renamed with `body.rename` keeps its own name (#3510).
                let mut name = match f.body_names.get(k).filter(|n| !n.trim().is_empty() && st.body(n).is_none()) {
                    Some(n) => n.clone(),
                    None => format!("{} ({})", mb.name, k + 1),
                };
                while st.body(&name).is_some() {
                    name.push('\'');
                }
                // A piece stays with the body, in its component and its frame (as the first piece,
                // which keeps the body's name, does), wherever the split was made.
                let mut piece = ModelBody::traced_new(name, b, f.id);
                piece.component = mb.component;
                piece.transferred = true;
                piece.source_transform = mb.source_transform;
                st.bodies.push(piece);
            }
            Ok(())
        }
        FeatureKind::Pattern { features, pattern, bodies, placements, components, .. } => {
            // Component placements are reconciled on the document after evaluation (#12116).
            if !components.is_empty() {
                let _ = pattern_transforms(doc, vals, st, f.component, pattern)?;
                return Ok(());
            }
            let mats = own_copies(doc, vals, st, f)?;
            if bodies.is_empty() {
                return replay(doc, vals, f, st, features, &mats, warning);
            }
            // Bodies: each copy is a new body named after its source. A body that cannot be
            // copied (gone, its placement gone, or its geometry refused by the move) is left out
            // with a warning: the others are still copied, and only a pattern that copies none
            // fails.
            let mut skipped = Vec::new();
            let mut left_out = Vec::new();
            let mut first_error = None;
            for (s, n) in bodies.iter().enumerate() {
                if let Err(e) = pattern_body(doc, f, st, &mats, bodies.len(), s, n, placements.get(s).map(Vec::as_slice)) {
                    skipped.push(n.to_string());
                    left_out.push(s);
                    first_error.get_or_insert(e);
                }
            }
            // Kept so the names stored for the left-out copies stay theirs when the session pins
            // the names the copies have now (`Feature::pattern_pins`).
            st.pattern_left_out.remove(&f.id);
            if !left_out.is_empty() {
                st.pattern_left_out.insert(f.id, left_out);
            }
            match first_error {
                Some(e) if skipped.len() == bodies.len() => Err(e),
                Some(e) => {
                    append_warning(warning, pattern_skipped_note(&skipped, &e));
                    Ok(())
                }
                None => Ok(()),
            }
        }
        FeatureKind::Mirror { features, plane, bodies, combine, placements } => {
            let m = mirror_copy(doc, vals, st, f, plane)?;
            if bodies.is_empty() {
                return replay(doc, vals, f, st, features, &[m], warning);
            }
            let mut output = 0;
            // Copies evaluated on each frame a placement is dragged, which nothing cuts or joins:
            // searching their revolved faces again was nearly all that took.
            let shown_only = !*combine && !placements.is_empty() && placement_reuse::copies_only_shown(doc, f);
            for (k, n) in bodies.iter().enumerate() {
                let i = st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                let Some(mb) = st.bodies.get(i).cloned() else { continue };
                // A body picked where a placement shows it is mirrored there: the copy goes beside
                // it into the component that placement shows, a joined body stays in its own frame.
                let (component, copy, own) = match mirrored_placement(doc, f.component, &mb, placements.get(k).map(Vec::as_slice))? {
                    Some((component, frame, source)) => {
                        let singular = || DocError::Invalid(format!("the placement `{n}` was mirrored at is singular"));
                        let inverse = |m: &Mat| crate::mat_inverse(m).ok_or_else(singular);
                        // The plane and both frames are in this feature's component: the body
                        // mirrored there, so where the component itself is placed does not count.
                        let mirrored = crate::mat_mul(&m, &source);
                        (Some(component), crate::mat_mul(&inverse(&frame)?, &mirrored), crate::mat_mul(&inverse(&source)?, &mirrored))
                    }
                    None => (None, m, m),
                };
                let copied = if shown_only {
                    kernel::place_matrix(&mb.body, copy)
                } else {
                    kernel::transform_matrix(&mb.body, if *combine { own } else { copy })
                };
                let mirrored = moved_copy(&mb, copied?, 1);
                if *combine {
                    let joined = crate::flush_join::join_bodies(&mb.body, &mirrored.body, "the mirror join produced nothing")?;
                    let joined = Traced::step(joined, &[&mb.traced(), &mirrored]);
                    let pieces = joined.body.lumps()?;
                    let lineages = crate::lineage::Lineage::lumps(&joined.body, joined.lineage.as_ref(), &pieces);
                    for (k, (p, lineage)) in pieces.into_iter().zip(lineages).enumerate() {
                        if k == 0 {
                            if let Some(slot) = st.bodies.get_mut(i) {
                                *slot = ModelBody::new(mb.name.clone(), p, mb.feature).with_lineage(lineage);
                            }
                        } else {
                            let name = copy_body_name(st, f, output, &mb.name);
                            st.bodies.push(ModelBody::new(name, p, f.id).with_lineage(lineage));
                            output += 1;
                        }
                    }
                } else {
                    let name = copy_body_name(st, f, output, &mb.name);
                    copy_sheet(st, &mb.name, &name, &copy);
                    let mut body = ModelBody::traced_new(name, mirrored, f.id);
                    if !placements.is_empty() {
                        body.mirrored_mesh = placed_mesh::PlacedMesh::mirrored(&mb, &body.body, &copy).map(Arc::new);
                    }
                    if let Some(c) = component.filter(|c| *c != f.component) {
                        // Kept in that component as a moved body is (and through later features).
                        body.component = c;
                        body.transferred = true;
                    }
                    st.bodies.push(body);
                    output += 1;
                }
            }
            Ok(())
        }
        FeatureKind::Fillet { edges, radius, body, .. } | FeatureKind::Chamfer { edges, distance: radius, body, .. } => {
            let r = val(vals, radius, Kind::Length)?;
            // A G2 corner is built by the corner construction alone, for a constant radius: any
            // other style (or a fitted radius) has no corner patch to repeat.
            if let FeatureKind::Fillet { corner: crate::FilletCorner::G2, style, fit, .. } = &f.kind
                && (!matches!(style, crate::FilletStyle::Constant) || !matches!(fit, crate::FilletFit::Exact))
            {
                return Err(DocError::Invalid("a G2 corner is built for a constant-radius fillet (type: constant, fit: exact) only".into()));
            }
            // Each edge is found on its own body (picked over several bodies, the edges of each
            // are blended on it), all against the bodies as they were before this feature.
            // Corner setbacks likewise: SMLib sets back the corners of constant-radius rounds.
            if let FeatureKind::Fillet { setbacks, style, fit, .. } = &f.kind
                && !setbacks.is_empty()
                && (!matches!(style, crate::FilletStyle::Constant) || !matches!(fit, crate::FilletFit::Exact))
            {
                return Err(DocError::Invalid("corner setbacks are built for a constant-radius fillet (type: constant, fit: exact) only".into()));
            }
            // Each picked edge at its own radius: one value per pick, for the blend that supports it.
            let each_r: Option<Vec<f64>> = match &f.kind {
                FeatureKind::Fillet { edge_radii, style, fit, corner, setbacks, turn, tangent_chain, .. } if !edge_radii.is_empty() => {
                    if !style.is_constant() || !fit.is_exact() || !corner.is_rolling() || !setbacks.is_empty() || !turn.is_mitre() || *tangent_chain {
                        return Err(DocError::Invalid(crate::EDGE_RADII_TAKE.into()));
                    }
                    if edge_radii.len() != edges.len() {
                        return Err(DocError::Invalid(format!(
                            "`edge_radii` gives {} radii for {} picked edges: one per edge, in the order of `edges`",
                            edge_radii.len(),
                            edges.len()
                        )));
                    }
                    let mut v = Vec::with_capacity(edge_radii.len());
                    for e in edge_radii {
                        v.push(val(vals, e, Kind::Length)?);
                    }
                    Some(v)
                }
                _ => None,
            };
            let (on, on_several) = blend_bodies(st, f, body, &crate::plain(edges))?;
            let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
            for (i, ti) in on.iter().enumerate() {
                match groups.iter_mut().find(|(b, _)| b == ti) {
                    Some((_, idx)) => idx.push(i),
                    None => groups.push((*ti, vec![i])),
                }
            }
            let (mut found, mut notes, mut fitted, mut tapered_all) = (Vec::new(), on_several, Vec::new(), Vec::new());
            let mut reduced = false;
            // The kernel blends exactly the edges given: the feature adds each one's tangent
            // chain when it asks for it. A radius law runs along one edge, so a variable or
            // spline round takes its edge alone.
            let chain = match &f.kind {
                FeatureKind::Fillet { tangent_chain, style, .. } => {
                    *tangent_chain && !matches!(style, crate::FilletStyle::Variable { .. } | crate::FilletStyle::Spline { .. })
                }
                FeatureKind::Chamfer { tangent_chain, .. } => *tangent_chain,
                _ => false,
            };
            let mut picks = Vec::with_capacity(groups.len());
            // Per-edge radii: each resolved edge point, on its body, keyed (exactly) to its pick and
            // radius. The smooth-edge filter below only drops picks, so the keys survive it.
            let key = |ti: usize, p: &Vec3| (ti, [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
            let mut radius_of: BTreeMap<(usize, [u64; 3]), (usize, f64)> = BTreeMap::new();
            // (pick, the blended edge's middle, its radius), for pattern and mirror copies.
            let mut by_pick: Vec<(usize, Vec3, f64)> = Vec::new();
            for (ti, idx) in &groups {
                let Some(mb) = st.bodies.get(*ti) else { return Err(DocError::Invalid("body".into())) };
                let mut w = None;
                let resolved = blend_edges_on(mb, f, edges, idx, &mut w)?;
                if let Some(all) = &each_r {
                    if resolved.len() != idx.len() {
                        return Err(DocError::Invalid(
                            "an edge given its own radius (`edge_radii`) no longer resolves to one edge: each radius needs its edge".into(),
                        ));
                    }
                    for (p, i) in resolved.iter().zip(idx) {
                        radius_of.insert(key(*ti, p), (*i, all.get(*i).copied().unwrap_or(r)));
                    }
                }
                picks.push((*ti, resolved));
                if let Some(w) = w {
                    notes.push(w);
                }
            }
            let mut blended = Vec::new();
            for (ti, edges) in without_smooth(st, picks, &mut notes)? {
                let Some(mb) = st.bodies.get(ti) else { return Err(DocError::Invalid("body".into())) };
                let picked = edges.len();
                let beyond = if chain { Ok(Vec::new()) } else { chain_beyond_picks(&mb.body, &edges) };
                let edges = if chain { kernel::tangent_chain(&mb.body, &edges)? } else { edges };
                let mut note = None;
                let propagated = edges.len() > picked;
                // This body's edges' own radii, found by their resolved points.
                let each: Option<Vec<f64>> =
                    each_r.as_ref().map(|_| edges.iter().map(|e| radius_of.get(&key(ti, e)).map_or(r, |(_, x)| *x)).collect());
                let (nb, used, tapered) = blend_body(vals, f, mb, &edges, r, each.as_deref(), &mut note)
                    .map_err(|e| if propagated { with_propagation_hint(e) } else { e })?;
                if let Some(n) = note {
                    notes.push(n);
                }
                if let Some(n) = left_sharp_note(&nb.body, &beyond, picked, &f.kind) {
                    notes.push(n);
                }
                reduced |= used.is_some();
                fitted.extend(used.unwrap_or_else(|| vec![r; edges.len()]));
                tapered_all.extend(tapered);
                // A pattern repeats each edge at a moved copy of a point on it: the edge's middle,
                // never the pick, which at a vertex two edges share finds either on a copy (#16934).
                let middles = kernel::blend_edge_middles(&mb.body, &edges)?;
                if each.is_some() {
                    // Per-edge radii: each blended edge's middle with its pick and radius.
                    by_pick.extend(edges.iter().zip(&middles).filter_map(|(e, m)| radius_of.get(&key(ti, e)).map(|(i, x)| (*i, *m, *x))));
                }
                found.extend(middles);
                blended.push((ti, nb));
            }
            if !notes.is_empty() {
                *warning = Some(notes.join("; "));
            }
            if each_r.is_some() {
                // The blended edges in pick order, each with its own radius: pattern and mirror
                // copies read the radius at the same index as the edge (as a fitted fillet's).
                by_pick.sort_by_key(|(i, _, _)| *i);
                found = by_pick.iter().map(|(_, m, _)| *m).collect();
                fitted = by_pick.iter().map(|(_, _, x)| *x).collect();
                reduced = true;
            }
            st.blend_edges.insert(f.id, found);
            if reduced {
                st.fitted.insert(f.id, fitted);
            }
            if !tapered_all.is_empty() {
                st.tapered.insert(f.id, tapered_all);
            }
            for (ti, nb) in blended {
                if let Some(slot) = st.bodies.get_mut(ti) {
                    let (name, feat) = (slot.name.clone(), slot.feature);
                    *slot = ModelBody::traced_new(name, nb, feat);
                }
            }
            Ok(())
        }
        FeatureKind::Box { corner, length, width, height, operation, centred } => {
            let s = Vec3::new(val(vals, length, Kind::Length)?, val(vals, width, Kind::Length)?, val(vals, height, Kind::Length)?);
            let corner = box_low_corner(point(vals, corner)?, s, *centred);
            let b = kernel::box_solid(corner, corner + s)?;
            let b = if matches!(operation, Operation::NewBody | Operation::Join) { b.with_box_certificate(f.id)? } else { b };
            apply_traced(doc, st, f, vec![crate::lineage::box_tool(b).with_recipe(tool_recipe(doc, f, vals))], *operation, &[])
        }
        FeatureKind::Cylinder { base, axis, radius, height, operation, .. } => {
            let axis = direction(vals, axis)?;
            let base = cylinder_base(vals, st, f, base, axis)?;
            let (radius, height) = (val(vals, radius, Kind::Length)?, val(vals, height, Kind::Length)?);
            let b = kernel::cylinder(base, axis, radius, height)?;
            let b = if matches!(operation, Operation::NewBody | Operation::Join) { b.with_cylinder_certificate(f.id)? } else { b };
            apply_traced(doc, st, f, vec![crate::lineage::cylinder_tool(b).with_recipe(tool_recipe(doc, f, vals))], *operation, &[])
        }
        FeatureKind::Sphere { center, radius, operation } => {
            let b = kernel::sphere(point(vals, center)?, val(vals, radius, Kind::Length)?)?;
            apply_op(doc, st, f, vec![b], *operation, &[])
        }
        FeatureKind::Torus { center, major, minor, operation } => {
            let b = kernel::torus(point(vals, center)?, val(vals, major, Kind::Length)?, val(vals, minor, Kind::Length)?)?;
            apply_op(doc, st, f, vec![b], *operation, &[])
        }
        FeatureKind::Cone { base, axis, radius, top_radius, height, operation } => {
            let (radius, top_radius, height) =
                (val(vals, radius, Kind::Length)?, val(vals, top_radius, Kind::Length)?, val(vals, height, Kind::Length)?);
            let tool = more::cone(point(vals, base)?, axis.0, radius, top_radius, height)?;
            apply_traced(doc, st, f, vec![tool], *operation, &[])
        }
        FeatureKind::Combine { target, tools, operation, keep_tools } => {
            if tools.iter().any(|t| t == target) {
                return Err(DocError::Invalid("a body cannot be combined with itself".into()));
            }
            // Each body is kept in its own component's frame: a tool placed differently from the
            // target is moved into the target's frame, so it is combined where it is shown (#3383).
            // A tool placed exactly as the target is left as it is.
            let target_world = st.body(target).map(|b| body_world(doc, b));
            // A tool of a further placement (`Drill (Hinge:2)`) is not in the model, which holds a
            // component's first placement only: it is placed where its occurrence puts it, in the
            // target's frame (#20040).
            let missing: Vec<&str> = tools.iter().map(|t| t.0.as_str()).filter(|t| st.body(t).is_none()).collect();
            let placed = match &target_world {
                Some(w) if !missing.is_empty() => {
                    with_placed_copies(doc, st, std::borrow::Cow::Owned(ModelState::default()), w, &missing).into_owned()
                }
                _ => ModelState::default(),
            };
            let mut tb = Vec::new();
            for t in tools {
                match st.body(t) {
                    Some(b) => {
                        let traced = b.traced();
                        let world = body_world(doc, b);
                        match target_world.filter(|w| *w != world) {
                            Some(w) => {
                                let into_target = crate::mat_inverse(&w)
                                    .ok_or_else(|| DocError::Invalid(format!("`{target}` is placed where it cannot be combined")))?;
                                let rel = crate::mat_mul(&into_target, &world);
                                tb.push(Traced::step(kernel::transform_matrix(&b.body, rel)?, &[&traced]));
                            }
                            None => tb.push(traced),
                        }
                    }
                    None if placed.body(t).is_some() => {
                        tb.extend(placed.body(t).map(ModelBody::traced));
                        // Removing it would remove the body from every placement of its component.
                        if !keep_tools {
                            let source = placed.body(t).and_then(|b| b.source_name.as_deref()).unwrap_or(t.0.as_str());
                            append_warning(warning, format!("`{t}` is kept: it is a placement of `{source}`, whose other placements show it"));
                        }
                    }
                    // Designs saved before #2247 combined the pieces a cut split off a body that
                    // was already several pieces (`Posts`, `Posts (1)`). A cut keeps them in one
                    // body now: the piece is in `Posts`, which is a tool too or the body joined to.
                    None => match split_piece_of(t) {
                        Some(base)
                            if st.body(base).is_some()
                                && (tools.iter().any(|o| **o == *base)
                                    || (**target == *base && matches!(operation, Operation::Join | Operation::NewBody))) =>
                        {
                            append_warning(warning, format!("`{t}` is part of `{base}` now"));
                        }
                        // A tool whose feature is suppressed is left out, as a suppressed feature is
                        // everywhere else: the Combine builds from the tools that are there (#13474).
                        // Only a feature before the Combine can have made a tool.
                        _ if doc
                            .features
                            .iter()
                            .take_while(|g| g.id != f.id)
                            .any(|g| g.suppressed && g.body_names.iter().any(|n| n.trim() == t.0.as_str())) =>
                        {
                            append_warning(warning, format!("`{t}` is left out: the feature that makes it is suppressed"));
                        }
                        _ => return Err(DocError::Unknown(format!("body `{t}`"))),
                    },
                }
            }
            if st.body(target).is_none() {
                return Err(DocError::Unknown(format!("body `{target}`")));
            }
            if tb.is_empty() && !tools.is_empty() {
                return Ok(()); // the pieces were all joined back into the body they are part of
            }
            let op = if *operation == Operation::NewBody { Operation::Join } else { *operation };
            // #15913, #18708: booleans require closed solids: an open tool body (or open target in join)
            // fails deep in kernel export with cryptic errors or swallows surfaces. Refuse it here,
            // naming the offending body in user terms before attempting the boolean.
            match op {
                Operation::Join => {
                    let surface =
                        std::iter::once(target).chain(tools.iter()).find(|n| st.body(n).or_else(|| placed.body(n)).is_some_and(|b| b.is_surface()));
                    if let Some(name) = surface {
                        return Err(DocError::Invalid(format!(
                            "{name} is a surface body, and a join needs solids: Thicken it first, or Stitch closed surfaces with Surface › Stitch"
                        )));
                    }
                }
                Operation::Cut => {
                    if let Some(name) = tools.iter().find(|n| st.body(n).or_else(|| placed.body(n)).is_some_and(|b| !b.is_closed())) {
                        return Err(DocError::Invalid(format!(
                            "\"{name}\" isn't a closed solid (it has open edges), so it can't cut. Close it with Stitch, or use a closed body as the tool"
                        )));
                    }
                }
                Operation::Intersect => {
                    if let Some(name) = tools.iter().find(|n| st.body(n).or_else(|| placed.body(n)).is_some_and(|b| !b.is_closed())) {
                        return Err(DocError::Invalid(format!(
                            "\"{name}\" isn't a closed solid (it has open edges), so it can't intersect. Close it with Stitch, or use a closed body as the tool"
                        )));
                    }
                }
                _ => {}
            }
            apply_traced(doc, st, f, tb, op, std::slice::from_ref(&target.0))?;
            if !keep_tools {
                st.bodies.retain(|b| !tools.iter().any(|t| *t == b.name));
            }
            Ok(())
        }
        FeatureKind::Scale { bodies, origin, factor, factors } => {
            let k = match factors {
                Some(fs) => [val(vals, &fs[0], Kind::Unitless)?, val(vals, &fs[1], Kind::Unitless)?, val(vals, &fs[2], Kind::Unitless)?],
                None => {
                    let x = val(vals, factor, Kind::Unitless)?;
                    [x, x, x]
                }
            };
            if k.iter().any(|x| !(x.is_finite() && *x > 1e-9 && *x < 1e6)) {
                return Err(DocError::Invalid("scale factors must be positive".into()));
            }
            let o = *origin;
            // Column-major: scale about the origin.
            let m = [
                [k[0], 0.0, 0.0, 0.0],
                [0.0, k[1], 0.0, 0.0],
                [0.0, 0.0, k[2], 0.0],
                [o.x * (1.0 - k[0]), o.y * (1.0 - k[1]), o.z * (1.0 - k[2]), 1.0],
            ];
            for n in bodies {
                let i = st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                if let Some(mb) = st.bodies.get_mut(i) {
                    let nb = Traced::step(kernel::transform_matrix(&mb.body, m)?, &[&mb.traced()]);
                    *mb = mb.remade(nb);
                }
            }
            Ok(())
        }
        FeatureKind::OffsetFace { faces, distance, body } => {
            let d = val(vals, distance, Kind::Length)?;
            let i = body_at(st, body, &crate::plain(faces))?;
            // A distance of 0 leaves the faces where they are, as Offset Plane does; a parameter
            // can pass through 0 without the design failing to rebuild (#20496).
            if d.abs() < 1e-9 {
                return Ok(());
            }
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let nb = Traced::step(kernel::offset_faces(&mb.body, &crate::plain(faces), d)?, &[&mb.traced()]);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = mb.remade(nb);
            }
            Ok(())
        }
        FeatureKind::DeleteFace { faces, body, heal } => {
            let i = body_at(st, body, &crate::plain(faces))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let mut picked: Vec<usize> = Vec::new();
            for p in faces {
                let fi = crate::appearance::face_index_at(&mb, p.0)
                    .ok_or_else(|| DocError::Invalid(format!("Delete Face: no face of {} at {}", mb.name, p.0)))?;
                if !picked.contains(&fi) {
                    picked.push(fi);
                }
            }
            let all = kernel::with_same_surface(&mb.body, &picked);
            let left = if *heal { kernel::delete_faces(&mb.body, &all)? } else { kernel::remove_faces(&mb.body, &all)? };
            let nb = Traced::step(left, &[&mb.traced()]);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = mb.remade(nb);
            }
            Ok(())
        }
        FeatureKind::FrameEndCap { .. } => crate::end_cap::evaluate(doc, vals, f, st),
        FeatureKind::FrameMemberTurn { .. } => frame_member::check_turn(doc, vals, f, st),
        FeatureKind::FrameGusset { .. } => crate::gusset::evaluate(doc, vals, f, st),
        FeatureKind::FrameMember { .. } => frame_member::evaluate(doc, vals, f, st),
        FeatureKind::FrameTrim { .. } => frame_member::trim(doc, vals, f, st, warning),
        FeatureKind::FramePlate { .. } => crate::frame_plate::evaluate(doc, vals, f, st),
        FeatureKind::BoundingSolid { bodies, margin, contents, margins } => {
            let m = val(vals, margin, Kind::Length)?;
            let side = |i: usize| margins.as_ref().and_then(|ms| ms.get(i)).map_or(Ok(m), |e| val(vals, e, Kind::Length));
            let (lo, hi) = (Vec3::new(side(0)?, side(2)?, side(4)?), Vec3::new(side(1)?, side(3)?, side(5)?));
            // An envelope smaller than its contents could miss a contact the parts make.
            if *contents && [lo.x, lo.y, lo.z, hi.x, hi.y, hi.z].iter().any(|v| *v < 0.0) {
                return Err(DocError::Invalid("an envelope's margin cannot be negative".into()));
            }
            let bb = if *contents {
                crate::envelope::contents_bounds(doc, st, f.component)?
            } else {
                // The bodies' display meshes, as before envelopes: a saved Bounding Solid builds
                // the same box it always did.
                let mut bb = solidcraft_geom::Aabb3::EMPTY;
                for b in st.bodies.iter().filter(|b| bodies.is_empty() || bodies.iter().any(|t| *t == b.name)) {
                    bb = bb.union(&b.mesh().bounds());
                }
                bb
            };
            if bb.is_empty() {
                return Err(DocError::Invalid(if *contents { "the component holds no bodies to bound".into() } else { "no bodies to bound".into() }));
            }
            let (min, max) = (bb.min - lo, bb.max + hi);
            if !(min.x < max.x && min.y < max.y && min.z < max.z) {
                return Err(DocError::Invalid("a side's margin reaches past the opposite side of the bodies".into()));
            }
            let b = crate::lineage::numbered_tool(kernel::box_solid(min, max)?);
            let name = unique_body_name(st, f.body_names.first().map(String::as_str).unwrap_or("Bounding"));
            let mut envelope = ModelBody::traced_new(name, b, f.id);
            envelope.envelope = *contents;
            st.bodies.push(envelope);
            Ok(())
        }
        FeatureKind::Move { bodies, translate, rotate_axis, rotate_about, origin, angle, copy, copy_frames } => {
            if !copy_frames.is_empty()
                && (!*copy || copy_frames.len() != bodies.len() || copy_frames.iter().any(|m| !crate::valid_transfer_matrix(m)))
            {
                return Err(DocError::Invalid("invalid Move copy placements".into()));
            }
            let t =
                Vec3::new(val(vals, &translate[0], Kind::Length)?, val(vals, &translate[1], Kind::Length)?, val(vals, &translate[2], Kind::Length)?);
            let ang = match angle {
                Some(a) => val(vals, a, Kind::Angle)?,
                None => 0.0,
            };
            let axis = rotate_axis.map(Vec3::from).unwrap_or(Vec3::Z);
            let origin = origin.as_ref().map(|p| point(vals, p)).transpose()?.unwrap_or(Vec3::ZERO);
            // A turn picked on an edge follows the edge where it is now (#17974).
            let (origin, axis) = match rotate_about {
                Some(about) => move_turn_line(doc, st, f.component, about, origin, axis, warning)?,
                None => (origin, axis),
            };
            // With `copy` set, the source body is kept in place and the transform lands on a new
            // body pushed beside it instead of replacing it.
            let mut output = 0;
            for n in bodies {
                let i = st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                let Some(mb) = st.bodies.get(i).cloned() else { continue };
                let nb = kernel::transform(&mb.body, t, origin, axis, ang)?;
                if *copy {
                    let mut motion = crate::rigid(t, origin, axis, ang);
                    let nb = if let Some(frame) = copy_frames.get(output).filter(|m| !crate::is_identity(m)) {
                        motion = crate::mat_mul(frame, &motion);
                        kernel::transform_matrix(&nb, *frame)?
                    } else {
                        nb
                    };
                    let traced = moved_copy(&mb, nb, 1);
                    let name = copy_body_name(st, f, output, &mb.name);
                    copy_sheet(st, &mb.name, &name, &motion);
                    st.bodies.push(ModelBody::traced_new(name, traced, f.id));
                    output += 1;
                } else if let Some(slot) = st.bodies.get_mut(i) {
                    *slot = mb.remade(Traced::step(nb, &[&mb.traced()]));
                }
            }
            if !*copy {
                move_constructions_with(doc, st, bodies, &crate::rigid(t, origin, axis, ang));
            }
            Ok(())
        }
        FeatureKind::MeshConvert { body, tolerance } => {
            let tolerance = val(vals, tolerance, Kind::Length)?;
            let index = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let source = st.bodies.get(index).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            if doc.model_body_component(source) != f.component {
                return Err(DocError::Invalid("activate the mesh body's component before converting it".into()));
            }
            let converted = kernel::convert_prismatic_mesh(&source.body, tolerance)?;
            let converted = converted.with_color(source.body.color());
            let result = ModelBody::new(source.name.clone(), converted, f.id);
            let target = st.bodies.get_mut(index).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            *target = result;
            Ok(())
        }
        FeatureKind::MeshReduce { body, triangle_count, percentage } => {
            let source = st.body(body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            if doc.model_body_component(source) != f.component {
                return Err(DocError::Invalid("activate the mesh body's component before reducing it".into()));
            }
            let (_, triangles) = source.body.triangle_mesh().ok_or_else(|| DocError::Invalid("select a mesh body".into()))?;
            let original = triangles.len();
            let target = match (triangle_count, percentage) {
                (Some(n), None) if *n > 0 => *n,
                (None, Some(p)) if p.is_finite() && *p > 0.0 && *p < 100.0 => ((original as f64 * p / 100.0).floor() as usize).max(1),
                _ => return Err(DocError::Invalid("specify exactly one positive triangle_count or percentage between 0 and 100".into())),
            };
            if target >= original {
                return Err(DocError::Invalid(format!("target {target} must be less than the original {original} triangles")));
            }
            let (result, achieved) = kernel::reduce_mesh(&source.body, target)?;
            if achieved >= original {
                return Err(DocError::Invalid("mesh reduction could not remove any triangles under its geometric and topology constraints".into()));
            }
            if achieved > target {
                *warning = Some(format!(
                    "reduction stopped at {achieved} triangles; requested {target}; geometric or topology constraints prevented further reduction"
                ));
            }
            let source_offset = doc.model_body_offset(source).copied();
            let name = copy_body_name(st, f, 0, &format!("{body} Reduced"));
            // A stored placement for the new body takes precedence. Otherwise materialize the
            // source offset in its geometry: no new document placement is needed.
            let result = match source_offset.filter(|_| !doc.body_offsets.contains_key(&name)) {
                Some(offset) => kernel::transform_matrix(&result, offset)?,
                None => result,
            };
            st.bodies.push(ModelBody::new(name, result, f.id));
            Ok(())
        }
        FeatureKind::Repair { body, gap, patch_holes } => {
            let gap = val(vals, gap, Kind::Length)?;
            let index = st.bodies.iter().position(|b| &b.name == body).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let source = st.bodies.get(index).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            let (repaired, report) = kernel::repair(&source.body, &kernel::RepairOptions { gap, patch_holes: *patch_holes, heal_first: heal })?;
            let repaired = repaired.with_color(source.body.color());
            let result = ModelBody::new(source.name.clone(), repaired, source.feature);
            let mut left: Vec<String> =
                report.refused.iter().map(|r| format!("SMLib still refuses face {} ({:?}): {}", r.face + 1, r.surface, r.why)).collect();
            left.extend(
                report
                    .holes
                    .iter()
                    .filter(|h| !h.patched)
                    .map(|h| h.why.clone().unwrap_or_else(|| format!("a hole of {} edges is left open", h.edges.len()))),
            );
            if !left.is_empty() {
                *warning = Some(left.join("; "));
            }
            let target = st.bodies.get_mut(index).ok_or_else(|| DocError::Unknown(format!("body `{body}`")))?;
            *target = result;
            *repair = Some(Arc::new(report));
            Ok(())
        }
        FeatureKind::MeshImport { meshes, .. } => {
            for (k, (name, b)) in mesh_bodies(meshes, prepared_meshes)?.into_iter().enumerate() {
                let name = copy_body_name(st, f, k, &name);
                st.bodies.push(ModelBody::new(name, b, f.id));
            }
            Ok(())
        }
        FeatureKind::Import { file, step, skip, records, .. } => {
            #[cfg(test)]
            cost_tests::count(|c| c.imports_read += 1);
            // A linked design's bodies left out of the file, and not read back when it opened.
            if records.is_none() && step.is_empty() && doc.links.values().any(|link| link.feature == f.id) {
                return Err(DocError::Invalid(format!(
                    "the bodies of the linked file {file} are not in this design: it was saved without them, and they have not been read from the file since; Update Links reads them"
                )));
            }
            let (imp, digest) = match records {
                Some(r) => (kernel::records_import_shared(r)?, crate::config::payload_digest(&kernel::records_text(r)?)),
                None => (kernel::step_import_shared(step)?, crate::config::payload_digest(step)),
            };
            let names = import_body_names(st, &f.body_names, &imp, skip);
            // The parts the import's checks healed, when the model has them ([`ImportDigests::heals`]).
            let heals = IMPORT_HEALS.with(|h| h.borrow().clone());
            for (k, (b, name)) in imp.bodies.iter().zip(names).enumerate() {
                let Some(name) = name else { continue };
                let carried = f.link_faces.get(k).map_or(&[][..], Vec::as_slice);
                let healed = heals.as_ref().and_then(|h| h.body(k));
                let body = healed.map_or(&b.body, |h| &h.body);
                let source = healed.map(|h| h.face_source.as_slice());
                let b = crate::lineage::linked_tool_from(body.clone().with_import_certificate(&digest, f.id)?, f.id, carried, source);
                for record in f.link_threads.get(k).into_iter().flatten() {
                    if !record.axis_point.is_finite()
                        || !record.start.is_finite()
                        || !record.end.is_finite()
                        || record.end <= record.start
                        || !(1..=8).contains(&record.starts)
                        || [record.major_diameter, record.radius, record.pitch].iter().any(|v| !v.is_finite() || *v <= 0.0)
                    {
                        return Err(DocError::Invalid("invalid cosmetic thread record in linked body".into()));
                    }
                    let mut thread = record.clone();
                    thread.feature = f.id;
                    thread.body.clone_from(&name);
                    st.threads.push(thread);
                }
                st.bodies.push(ModelBody::traced_new(name, b, f.id));
            }
            // SMLib's check of the parts may still be running: the report holds it, and what it
            // finds joins the warning when it ends ([`Model::settle_import_checks`]).
            *warning = import_warning(&imp.warnings);
            if warning.is_some() || !imp.report.checks.is_empty() {
                *import_report = Some(Arc::new(imp.report.clone()));
            }
            Ok(())
        }
    }
}

/// An import's warning on its feature: its first three warnings, and how many more.
pub(crate) fn import_warning<S: AsRef<str>>(warnings: &[S]) -> Option<String> {
    let n = warnings.len();
    if n == 0 {
        return None;
    }
    let shown: Vec<&str> = warnings.iter().take(3).map(AsRef::as_ref).collect();
    let more = if n > 3 { format!(" (+{} more)", n - 3) } else { String::new() };
    Some(format!("{}{more}", shown.join("; ")))
}

/// A sketch feature: its links refreshed against the model before it, solved, its profiles
/// found. `lost_names` says what a link whose named face or edge is gone does: the evaluation
/// fails the sketch ([`crate::project::LostNames::Fail`]); a failed sketch shown for editing
/// marks the link lost instead ([`Model::hold_failed_sketch`]).
#[allow(clippy::too_many_arguments)]
fn eval_sketch(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    plane: &PlaneRef,
    sketch: &solidcraft_sketch::Sketch,
    st: &mut ModelState,
    warning: &mut Option<String>,
    lost_names: crate::project::LostNames,
) -> Result<()> {
    let anchor = crate::face_anchor::plane_face_anchor(plane);
    let fst = match anchor {
        Some(anchor) => crate::state_in_face_anchor_frame(doc, st, anchor, f.component)?,
        // A sketch that reads no geometry (a fixed plane, no projections) needs no other
        // component's bodies moved into its frame: placing every body of a large
        // assembly again is what made each sketch edit as slow as the design is big.
        None if !plane_reads_geometry(plane) && sketch.links.is_empty() => std::borrow::Cow::Borrowed(st),
        None => state_in_frame(doc, st, f.component)?,
    };
    // A further placement a link reads is placed in its frame too (#9957).
    let named: Vec<&str> = sketch.links.iter().flat_map(|l| l.source.bodies()).collect();
    let fst = match anchor.map(|anchor| anchor.author_frame(doc, f.component)) {
        _ if named.is_empty() => fst,
        Some(Ok(frame)) => with_placed_copies(doc, st, fst, &frame, &named),
        Some(Err(_)) => fst,
        None => with_placed_copies(doc, st, fst, &doc.component_transform(f.component), &named),
    };
    let (plane, face_warning) = match (anchor, plane) {
        (Some(_), plane) => doc.resolve_attached_plane_noted(vals, plane, f.component, st, &fst)?,
        // A flat face of a sheet by the sheet's name for it: no such face now.
        (None, plane) if crate::sheet::plane_sheet_face(plane).is_some() => return Err(lost_sheet_face(plane)),
        (None, PlaneRef::Face { plane: picked, at, name, anchor: None }) => face_plane(&fst, picked, *at, name.as_deref())?,
        (None, other) => {
            (doc.resolve_plane_in(vals, &*construction_planes_in_frame(doc, st, f.component, other)?, 0, f.component, Some(&fst))?, None)
        }
    };
    let mut sk = sketch.clone();
    // A text's outlines are rebuilt from its numbers when the links are refreshed: the
    // ones its expressions give come first.
    doc.apply_text_values(vals, &mut sk)?;
    // The frame the sketch is evaluated in, where a construction point it projects is
    // brought from its own component's (#12075).
    let link_frame = match anchor.map(|anchor| anchor.author_frame(doc, f.component)) {
        Some(Ok(frame)) => frame,
        _ => doc.component_transform(f.component),
    };
    let mut warns = crate::project::refresh_links_with(doc, vals, &fst, &plane, &mut sk, anchor, lost_names, Some(&(f.component, link_frame)))?;
    drop(fst);
    sk.refresh_offsets()?;
    if let Some(w) = face_warning {
        warns.insert(0, w);
    }
    // The stored sketch is solved for its stored values, with its links where they
    // were stored: a link that moved must not change the side a dimension from it is on.
    let stored = sk.driving_values();
    let mut report = doc.solve_sketch(vals, &mut sk, &stored, Some(sketch))?;
    doc.discount_traces(f.id, &sk, &mut report);
    if !report.keeps_orientation() {
        warns.insert(0, report.failure(|f| format!("the sketch constraints conflict ({f})")));
    } else {
        // The solve may have moved the curve a text runs along.
        warns.extend(crate::project::relay_path_texts(&mut sk));
    }
    // 3D constraints that fail leave the 3D curves where they were (#10608).
    if let Some(w) = report.wires.as_ref().filter(|w| !w.ok) {
        warns.push(w.failure(|f| format!("the 3D sketch constraints conflict ({f})")));
    }
    if !warns.is_empty() {
        *warning = Some(warns.join("; "));
    }
    sk.validate_periodic_splines()?;
    // A group of touching curves too large to search whole says so, rather than
    // leaving profiles out silently (#6896).
    let search = solidcraft_sketch::find_profiles_checked(&sk);
    if search.incomplete {
        append_warning(warning, profiles_cut_short_note());
    }
    let profiles = search.profiles;
    let profiles_3d = solidcraft_sketch::find_planar_3d_profiles(&sk, plane);
    let ends = solidcraft_sketch::loose_ends(&sk);
    st.sketches.retain(|s| s.feature != f.id);
    st.sketches.push(Arc::new(SolvedSketch { feature: f.id, name: f.name.clone(), plane, sketch: sk, report, profiles, profiles_3d, ends }));
    Ok(())
}

/// `copy`, a moved copy of `source` (instance `instance` of a pattern or mirror): its faces are
/// named after the source's.
fn moved_copy(source: &ModelBody, copy: Body, instance: usize) -> Traced {
    Traced::step(copy, &[&source.before_as(crate::lineage::Operand::Copy { body: source.name.clone(), instance })])
}

/// Meshes validated by the import reader for a newly added MeshImport feature.
#[derive(Clone, Debug)]
pub struct PreparedMeshImport(Vec<(crate::MeshData, Body)>);

impl PreparedMeshImport {
    /// Build the exact inputs that the timeline would use. Prepare before adding the feature
    /// so a malformed source is reported as an import error.
    pub fn build(meshes: &[crate::MeshData]) -> Result<Self> {
        let bodies: Result<Vec<_>> = meshes.iter().map(|m| Ok((m.clone(), build_import_mesh(m)?))).collect();
        bodies.map(Self)
    }

    /// Keep an indexed source for the flat mesh that the STL reader has just validated.
    /// The feature and its prepared body share the same source identity.
    pub fn build_indexed_flat(mesh: &mut crate::MeshData) -> Result<Self> {
        if !mesh.corner_normals.is_empty() {
            return Self::build(std::slice::from_ref(mesh));
        }
        let crate::MeshData { name, positions, triangles, .. } = mesh;
        let (body, indexed_positions, indexed_triangles) = Body::from_packed_flat_mesh_indexed(std::mem::take(positions), std::mem::take(triangles))
            .map_err(|e| DocError::Invalid(format!("mesh `{name}`: {e}")))?;
        *positions = indexed_positions.into();
        *triangles = indexed_triangles.into();
        Ok(Self(vec![(mesh.clone(), body)]))
    }

    fn body(&self, index: usize, mesh: &crate::MeshData) -> Option<&Body> {
        let (input, body) = self.0.get(index)?;
        // The command can generate different shading normals before it adds the feature.
        // Keep the input arrays to prove that the body was built from these exact values.
        (Arc::ptr_eq(&input.positions, &mesh.positions)
            && Arc::ptr_eq(&input.triangles, &mesh.triangles)
            && Arc::ptr_eq(&input.corner_normals, &mesh.corner_normals))
        .then_some(body)
    }
}

fn build_import_mesh(mesh: &crate::MeshData) -> Result<Body> {
    if !mesh.corner_normals.is_empty() && mesh.corner_normals.len() != mesh.triangles.len().saturating_mul(3) {
        return Err(DocError::Invalid(format!("mesh `{}`: invalid shading normal count", mesh.name)));
    }
    Body::from_packed_mesh(mesh.positions.as_chunks::<3>().0, mesh.triangles.as_chunks::<3>().0, mesh.corner_normals.as_chunks::<9>().0)
        .map_err(|e| DocError::Invalid(format!("mesh `{}`: {e}", mesh.name)))
}

/// Mesh bodies of a MeshImport feature.
fn mesh_bodies(meshes: &[crate::MeshData], prepared: Option<&PreparedMeshImport>) -> Result<Vec<(String, Body)>> {
    meshes
        .iter()
        .enumerate()
        .map(|(index, mesh)| {
            let body = match prepared.and_then(|prepared| prepared.body(index, mesh)) {
                Some(body) => body.clone(),
                None => build_import_mesh(mesh)?,
            };
            Ok((mesh.name.clone(), body.with_color(mesh.color)))
        })
        .collect()
}

/// The names an Import feature gives the file's bodies, added after the bodies of `st`, in the
/// file's order: `None` for a body it skips (a later instance of a repeated part). `body_names`
/// are the feature's stored names (by index), else the file's own; the ones past the file's
/// bodies are kept for solids the reader left out.
pub fn import_body_names(st: &ModelState, body_names: &[String], imp: &kernel::StepImport, skip: &[u64]) -> Vec<Option<String>> {
    let skip: std::collections::HashSet<u64> = skip.iter().copied().collect();
    // Files from board and assembly tools give thousands of bodies the same name: name them
    // from a set, not by searching the bodies for every candidate (cubic).
    let mut names = BodyNames::new(st);
    // Names kept after the file's bodies belong to solids this reader leaves out (#16857): no
    // other body takes them.
    for kept in body_names.iter().skip(imp.bodies.len()).map(|n| n.trim()).filter(|n| !n.is_empty()) {
        names.taken.insert(kept.to_string());
    }
    imp.bodies
        .iter()
        .enumerate()
        .map(|(k, b)| {
            if skip.contains(&(k as u64)) {
                return None;
            }
            let base = body_names.get(k).filter(|n| !n.trim().is_empty()).unwrap_or(&b.name);
            Some(names.unique(base))
        })
        .collect()
}

/// [`unique_body_name`] for many bodies in a row: the same names, in time linear in the bodies.
struct BodyNames {
    taken: std::collections::HashSet<String>,
    /// The next suffix worth trying for each base (smaller ones are all taken).
    next: std::collections::HashMap<String, usize>,
}

impl BodyNames {
    fn new(st: &ModelState) -> Self {
        BodyNames { taken: st.bodies.iter().map(|b| b.name.clone()).collect(), next: std::collections::HashMap::new() }
    }
    fn unique(&mut self, base: &str) -> String {
        let base = if base.trim().is_empty() { "Body" } else { base.trim() };
        if self.taken.insert(base.to_string()) {
            return base.to_string();
        }
        let mut i = self.next.get(base).copied().unwrap_or(2);
        let name = loop {
            let candidate = format!("{base} ({i})");
            i += 1;
            if self.taken.insert(candidate.clone()) {
                break candidate;
            }
        };
        self.next.insert(base.to_string(), i);
        name
    }

    /// The first free `base (i)` from `from` on, taken now.
    fn first_free(&mut self, base: &str, from: u64) -> String {
        let mut i = from.max(self.next.get(base).map_or(0, |n| *n as u64));
        loop {
            let candidate = format!("{base} ({i})");
            i += 1;
            if self.taken.insert(candidate.clone()) {
                return candidate;
            }
        }
    }
}

/// `base`, or `base (2)`, `base (3)`… if a body already has that name.
fn unique_body_name(st: &ModelState, base: &str) -> String {
    let base = if base.trim().is_empty() { "Body" } else { base.trim() };
    if st.body(base).is_none() {
        return base.to_string();
    }
    (2..).map(|i| format!("{base} ({i})")).find(|n| st.body(n).is_none()).unwrap_or_else(|| base.to_string())
}

/// The model as a component sees it: bodies and sketches of components placed differently from
/// it are moved into its frame (through both placements), so a sketch on another component's
/// face finds the face where it is shown and projects its edges there. The model itself when
/// every placement agrees (the usual case).
/// What construction geometry of component `comp` is built on: the model as that component sees
/// it, other components' bodies where they are placed. In the root, every placement of a
/// component (its pasted instances too) is there, as the viewport shows it.
fn construction_state<'a>(doc: &Document, st: &'a ModelState, comp: u64) -> Result<std::borrow::Cow<'a, ModelState>> {
    if comp == 0 && !doc.occurrences.is_empty() {
        return Ok(std::borrow::Cow::Owned(world_state(doc, st)?));
    }
    state_in_frame(doc, st, comp)
}

/// The line a Move's turn picked on the edge or round face `about` turns about now: where `about`
/// is in the model before the move, pointing the way the saved `axis` does, so the turn keeps its
/// sense (#17974). `origin` and `axis` are the line saved when it was picked.
fn move_turn_line(
    doc: &Document,
    st: &ModelState,
    comp: u64,
    about: &crate::construct::GeoRef,
    origin: Vec3,
    axis: Vec3,
    warning: &mut Option<String>,
) -> Result<(Vec3, Vec3)> {
    let found = construction_state(doc, st, comp).and_then(|cs| crate::construct::model_axis_in(Some(doc), &cs, about, comp));
    match found {
        Ok((o, d)) => match solidcore_ops::affine::same_sense(d.into(), axis.into()).filter(|_| o.is_finite()) {
            Some(d) => Ok((o, Vec3::from(d))),
            None => lost_turn_line(origin, axis, "it has no direction".into(), warning),
        },
        Err(e) => lost_turn_line(origin, axis, e.to_string(), warning),
    }
}

/// What a Move does when the edge its turn was picked on is not found: it turns about the line
/// saved when it was picked and says so (#17974). The one place to change to refuse instead.
fn lost_turn_line(origin: Vec3, axis: Vec3, why: String, warning: &mut Option<String>) -> Result<(Vec3, Vec3)> {
    append_warning(warning, format!("the edge this turn was picked on was not found ({why}): it turns about where that edge was"));
    Ok((origin, axis))
}

/// Construction axes and points built on bodies a Move moves go with them (#10127): an axis
/// through a cylinder turns with the cylinder, as its faces do. Only those built from the moved
/// bodies alone (`Construct::bodies`) follow; one also held by a fixed point, a plane or another
/// body stays. `m` is the move in the bodies' own frame; each construction is kept in its
/// component's frame (`construction_state`), so the move is carried over into that frame.
fn move_constructions_with(doc: &Document, st: &mut ModelState, moved: &[crate::BodyName], m: &Mat) {
    let frame_of = |feature: u64| {
        let comp = doc.feature(feature).map(|f| f.component).unwrap_or(0);
        if comp == 0 && !doc.occurrences.is_empty() { crate::IDENTITY } else { doc.component_transform(comp) }
    };
    let world: BTreeMap<&str, Mat> =
        st.bodies.iter().filter(|b| moved.iter().any(|n| *n == b.name)).map(|b| (b.name.as_str(), body_world(doc, b))).collect();
    let mut placed = Vec::new();
    for (k, c) in st.construct.iter().enumerate() {
        let Some(body) = c.bodies.first().and_then(|b| world.get(b.as_str())) else { continue };
        if !c.bodies.iter().all(|b| world.contains_key(b.as_str())) {
            continue;
        }
        let frame = frame_of(c.feature);
        let (Some(body_inv), Some(frame_inv)) = (crate::mat_inverse(body), crate::mat_inverse(&frame)) else { continue };
        let in_frame = crate::mat_mul(&crate::mat_mul(&frame_inv, body), &crate::mat_mul(m, &crate::mat_mul(&body_inv, &frame)));
        placed.push((k, c.geom.placed(&in_frame)));
    }
    for (k, geom) in placed {
        if let Some(c) = st.construct.get_mut(k) {
            c.geom = geom;
        }
    }
}

/// Whether `plane` is given by the origin planes and numbers alone.
fn plane_is_fixed(plane: &PlaneRef) -> bool {
    match plane {
        PlaneRef::Origin { .. } | PlaneRef::Custom { .. } | PlaneRef::Placed { .. } => true,
        PlaneRef::AtAngle { line: Some(_), .. } | PlaneRef::AtAngle { pivot: Some(_), .. } => false,
        PlaneRef::Offset { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => plane_is_fixed(base),
        _ => false,
    }
}

/// Whether resolving `plane` reads the model's bodies or sketches (a face, edges, points),
/// rather than only the origin planes, numbers and construction planes.
fn plane_reads_geometry(plane: &PlaneRef) -> bool {
    match plane {
        PlaneRef::Origin { .. } | PlaneRef::Custom { .. } | PlaneRef::Datum { .. } | PlaneRef::Placed { .. } | PlaneRef::Construction { .. } => false,
        PlaneRef::AtAngle { line: Some(_), .. } | PlaneRef::AtAngle { pivot: Some(_), .. } => true,
        PlaneRef::Offset { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => plane_reads_geometry(base),
        PlaneRef::ThroughPoint { .. }
        | PlaneRef::Midplane { .. }
        | PlaneRef::TwoEdges { .. }
        | PlaneRef::AxisPoint { .. }
        | PlaneRef::ThreePoints { .. }
        | PlaneRef::Tangent { .. }
        | PlaneRef::AlongPath { .. }
        | PlaneRef::SquareToLine { .. }
        | PlaneRef::Perpendicular { .. }
        | PlaneRef::Face { .. } => true,
    }
}

/// Where a body's geometry is placed in the world: through its component's placement and any
/// offset it was moved into that component with (as [`state_in_frame_at`] places it).
fn body_world(doc: &Document, mb: &ModelBody) -> Mat {
    let m = doc.component_transform(doc.model_body_component(mb));
    doc.model_body_offset(mb).map(|offset| crate::mat_mul(&m, offset)).unwrap_or(m)
}

thread_local! {
    /// While a feature whose tools are drawn in its own component's frame is evaluated: each body
    /// placed differently, with the matrix from that frame into the body's own (#3383).
    static TOOL_FRAMES: std::cell::RefCell<Vec<(String, Mat)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Whether a feature joins, cuts or intersects with tools drawn in its own component's frame: a
/// primitive, or a solid built from its sketches. (A combine's tools are bodies, each in its own
/// frame; it moves them itself.)
fn drawn_tools_change_bodies(kind: &FeatureKind) -> bool {
    match kind {
        FeatureKind::Box { operation, .. }
        | FeatureKind::Cylinder { operation, .. }
        | FeatureKind::Sphere { operation, .. }
        | FeatureKind::Torus { operation, .. }
        | FeatureKind::Cone { operation, .. }
        | FeatureKind::Extrude { operation, .. }
        | FeatureKind::Revolve { operation, .. }
        | FeatureKind::Loft { operation, .. }
        | FeatureKind::Sweep { operation, .. }
        | FeatureKind::Pipe { operation, .. }
        | FeatureKind::Coil { operation, .. } => *operation != Operation::NewBody,
        FeatureKind::CellGrid { .. } => true,
        _ => false,
    }
}

/// The component whose frame a feature's drawn tools are made in: its own for a feature with
/// drawn tools (see [`drawn_tools_change_bodies`]), and its sketch's for a hole at sketch points,
/// whose points are kept in that sketch's frame (#17623), and for a rib (#17655). `None` for every other feature: a hole
/// at a picked position is kept in the frame of the body it was picked on.
fn tools_component(doc: &Document, f: &Feature) -> Option<u64> {
    match &f.kind {
        FeatureKind::Hole { points: Some(sp), .. } => doc.feature(sp.sketch.0).map(|s| s.component),
        FeatureKind::Rib { sketch, .. } => doc.feature(sketch.0).map(|s| s.component),
        kind if drawn_tools_change_bodies(kind) => Some(f.component),
        _ => None,
    }
}

/// The frames of [`TOOL_FRAMES`] for the feature being evaluated; what was there before is put
/// back when it is dropped.
struct ToolFrames(Vec<(String, Mat)>);

impl ToolFrames {
    /// For a feature with drawn tools (see [`tools_component`]): the bodies placed differently
    /// from the component its tools are made in, so they meet them where they are shown (#3383).
    fn install(doc: &Document, f: &Feature, st: &ModelState) -> Option<ToolFrames> {
        if doc.occurrences.is_empty() && doc.body_offsets.is_empty() {
            return None;
        }
        let frame = doc.component_transform(tools_component(doc, f)?);
        let frames: Vec<(String, Mat)> = st
            .bodies
            .iter()
            .filter_map(|b| {
                let world = body_world(doc, b);
                (world != frame).then(|| crate::mat_inverse(&world).map(|inv| (b.name.clone(), crate::mat_mul(&inv, &frame)))).flatten()
            })
            .collect();
        if frames.is_empty() {
            return None;
        }
        Some(ToolFrames(TOOL_FRAMES.with(|t| std::mem::replace(&mut *t.borrow_mut(), frames))))
    }
}

impl Drop for ToolFrames {
    fn drop(&mut self) {
        let before = std::mem::take(&mut self.0);
        TOOL_FRAMES.with(|t| *t.borrow_mut() = before);
    }
}

/// The matrix taking the evaluated feature's drawn tools into body `name`'s frame (`None`: the
/// same frame).
fn tool_frame(name: &str) -> Option<Mat> {
    TOOL_FRAMES.with(|t| t.borrow().iter().find(|(n, _)| n == name).map(|(_, m)| *m))
}

/// The model as the evaluated feature's drawn tools meet it: the bodies it changes (`targets`,
/// else the feature's participants; `*` for all) that are placed differently, moved into its
/// component's frame. A through-all extent, and every test of where a tool meets its targets, is
/// measured on it, so a target in another, moved component is met where it is shown (#3383).
/// The model itself when no such body is placed differently.
fn targets_as_shown<'a>(st: &'a ModelState, f: &Feature, targets: &[String]) -> Result<std::borrow::Cow<'a, ModelState>> {
    let names: &[String] = if targets.is_empty() { &f.participants } else { targets };
    let all = names.iter().any(|n| n == "*");
    let mut out: Option<ModelState> = None;
    for (i, b) in st.bodies.iter().enumerate() {
        if !(all || names.contains(&b.name)) {
            continue;
        }
        let Some(back) = tool_frame(&b.name).and_then(|m| crate::mat_inverse(&m)) else { continue };
        let shown = out.get_or_insert_with(|| st.clone());
        let mut moved = ModelBody::new(b.name.clone(), kernel::transform_matrix(&b.body, back)?, b.feature);
        moved.component = b.component;
        moved.transferred = b.transferred;
        moved.names = crate::naming::NameCell::same(b);
        if let Some(slot) = shown.bodies.get_mut(i) {
            *slot = moved;
        }
    }
    Ok(match out {
        Some(shown) => std::borrow::Cow::Owned(shown),
        None => std::borrow::Cow::Borrowed(st),
    })
}

/// `tool` moved by `m` (unchanged for `None`).
fn tool_in(tool: &Traced, m: Option<Mat>) -> Result<Traced> {
    match m {
        Some(m) => Ok(Traced::step(kernel::transform_matrix(&tool.body, m)?, &[tool])),
        None => Ok(tool.clone()),
    }
}

pub fn state_in_frame<'a>(doc: &Document, st: &'a ModelState, comp: u64) -> Result<std::borrow::Cow<'a, ModelState>> {
    state_in_frame_at(doc, st, comp, doc.component_transform(comp))
}

/// The model a feature's face points (`FeatureKind::face_points`) are found in. Extrude's To /
/// From and Revolve's To place geometry, so they are kept in the feature's own component frame
/// (`comp`): the model as that component sees it, a target in another, moved component where it
/// is shown (#928). Other face points are picks, kept in the frame of the body they lie on: the
/// model itself.
pub fn face_point_state<'a>(doc: &Document, st: &'a ModelState, kind: &FeatureKind, comp: u64) -> Result<std::borrow::Cow<'a, ModelState>> {
    match kind {
        FeatureKind::Extrude { extent, .. } => state_in_frame_pinned_to(doc, st, comp, &extent.picked_placements),
        FeatureKind::Revolve { .. } => state_in_frame(doc, st, comp),
        _ => Ok(std::borrow::Cow::Borrowed(st)),
    }
}

/// Whether a face is a plane that runs along `n` (its normal square to `n`).
fn plane_along(st: &ModelState, (bi, fi): (usize, usize), n: Vec3) -> bool {
    st.bodies.get(bi).is_some_and(
        |b| matches!(kernel::face_surface(&b.body, fi), Some(kernel::FaceSurface::Plane { normal, .. }) if normal.vec().dot(n).abs() < 1e-6),
    )
}

/// The direction an extrude runs along: its sketch plane's normal in the feature's frame.
fn extrude_normal(doc: &Document, st: &ModelState, kind: &FeatureKind, comp: u64) -> Option<Vec3> {
    match kind {
        FeatureKind::Extrude { sketch, .. } => {
            st.sketch(sketch.0).and_then(|ss| sketch_in_feature_frame(doc, ss, comp).ok()).map(|ss| ss.plane.normal())
        }
        _ => None,
    }
}

/// The names of the faces a feature's face points lie on (empty where none), found in
/// [`face_point_state`]. A To / From point is named after the nearest face over every body, the
/// one `object_height` resolves it to, not the first body with a face within tolerance: a part
/// resting a hair from the picked face would otherwise take its name. Another feature's point is
/// named on the bodies of the feature's own component first: each component's bodies lie in its
/// own coordinates there, where a part inserted twice has a face at the same point in each.
/// A feature that names the body whose faces it picks ([`FeatureKind::face_body`]) names each
/// point on that body first: another body with a face a hair from the pick (#16871) would
/// otherwise take the name, and evaluation would move the pick onto that body's face.
pub fn face_point_names(doc: &Document, st: &ModelState, kind: &FeatureKind, comp: u64) -> Result<Vec<String>> {
    let pts = kind.face_points();
    if thread_target_bodies(doc, st, kind)?.is_some() {
        let mut names = Vec::with_capacity(pts.len());
        for (slot, point) in pts.iter().enumerate() {
            let scoped;
            let input = if let Some(body) = kind.thread_target_body(slot) {
                scoped = ModelState { bodies: st.bodies.iter().filter(|b| b.name == body).cloned().collect(), ..ModelState::default() };
                &scoped
            } else {
                st
            };
            names.push(crate::naming::face_names_at(input, std::slice::from_ref(point)).pop().unwrap_or_default());
        }
        return Ok(names);
    }
    if !matches!(kind, FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. }) {
        let named =
            kind.face_body().map(|n| ModelState { bodies: st.bodies.iter().filter(|b| b.name == n).cloned().collect(), ..ModelState::default() });
        let on_named: Vec<String> = match &named {
            Some(one) if !one.bodies.is_empty() => crate::naming::face_names_at(one, &pts),
            _ => vec![String::new(); pts.len()],
        };
        let own = ModelState { bodies: st.bodies.iter().filter(|b| doc.model_body_component(b) == comp).cloned().collect(), ..ModelState::default() };
        return Ok(on_named
            .into_iter()
            .zip(&pts)
            .map(|(name, p)| {
                if !name.is_empty() {
                    return name;
                }
                let name = crate::naming::face_names_at(&own, std::slice::from_ref(p)).pop().unwrap_or_default();
                if name.is_empty() { crate::naming::face_names_at(st, std::slice::from_ref(p)).pop().unwrap_or_default() } else { name }
            })
            .collect());
    }
    let fst = face_point_state(doc, st, kind, comp)?;
    let normal = extrude_normal(doc, st, kind, comp);
    Ok(pts
        .iter()
        .map(|p| {
            let hit = fst
                .bodies
                .iter()
                .enumerate()
                .filter(|(_, b)| !b.body.is_mesh())
                .filter_map(|(bi, b)| {
                    let (d, fi) = nearest_object_face(b, *p, normal)?;
                    (d <= (b.body.size() * 1e-3).max(1e-3)).then_some((d, bi, fi))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let Some((_, bi, fi)) = hit else { return String::new() };
            // A vertex or an edge point names no face: the name would move the point onto one of
            // the faces that meet there when the design is rebuilt (#12968).
            if normal.is_some_and(|n| on_edge_without_cap(&fst, (bi, fi), *p, n)) {
                return String::new();
            }
            fst.bodies.get(bi).and_then(|b| crate::naming::face_names(b).get(fi).cloned()).unwrap_or_default()
        })
        .collect())
}

/// As `state_in_frame`, using the exact active placement for its shared source geometry.
pub fn state_in_frame_at<'a>(doc: &Document, st: &'a ModelState, comp: u64, frame: Mat) -> Result<std::borrow::Cow<'a, ModelState>> {
    state_in_frame_pinned(doc, st, comp, frame, &[])
}

/// As [`state_in_frame`], with each component `pins` names held at the placement relative to
/// `comp` it had when the feature's To / From Object was picked (#16732): a joint that slides
/// another component does not stretch what the pick measures to. Components without a pin are
/// where they are shown.
pub fn state_in_frame_pinned_to<'a>(
    doc: &Document,
    st: &'a ModelState,
    comp: u64,
    pins: &[crate::PickedPlacement],
) -> Result<std::borrow::Cow<'a, ModelState>> {
    state_in_frame_pinned(doc, st, comp, doc.component_transform(comp), pins)
}

fn state_in_frame_pinned<'a>(
    doc: &Document,
    st: &'a ModelState,
    comp: u64,
    frame: Mat,
    pins: &[crate::PickedPlacement],
) -> Result<std::borrow::Cow<'a, ModelState>> {
    use std::borrow::Cow;
    if doc.occurrences.is_empty() && doc.body_offsets.is_empty() {
        return Ok(Cow::Borrowed(st));
    }
    let inv = crate::mat_inverse(&frame).ok_or_else(|| DocError::Invalid(format!("component {comp}'s frame cannot be inverted")))?;
    let comp_of_feature = |id: u64| doc.feature(id).map(|f| f.component).unwrap_or(0);
    let rel = |c: u64, body: Option<&str>| {
        let pinned = if c == comp { None } else { pins.iter().find(|p| p.component == c) };
        let m = match pinned {
            Some(p) => p.placement,
            None => crate::mat_mul(&inv, &if c == comp { frame } else { doc.component_transform(c) }),
        };
        match body.and_then(|b| doc.body_offsets.get(b)) {
            Some(o) => crate::mat_mul(&m, o),
            None => m,
        }
    };
    let body_m: Vec<Mat> = st
        .bodies
        .iter()
        .map(|b| {
            let m = rel(doc.model_body_component(b), None);
            doc.model_body_offset(b).map(|offset| crate::mat_mul(&m, offset)).unwrap_or(m)
        })
        .collect();
    let sketch_m: Vec<Mat> = st.sketches.iter().map(|s| rel(comp_of_feature(s.feature), None)).collect();
    let world_wires_need_mapping = !crate::is_identity(&inv)
        && st.sketches.iter().any(|sketch| sketch.sketch.wires.iter().any(|wire| wire.space == solidcraft_sketch::WireSpace::World));
    if body_m.iter().chain(&sketch_m).all(crate::is_identity) && !world_wires_need_mapping {
        return Ok(Cow::Borrowed(st));
    }
    let mut out = st.clone();
    for (b, m) in out.bodies.iter_mut().zip(&body_m) {
        if !crate::is_identity(m) {
            let moved = st.body_in_frame(b, m).map_err(|e| DocError::Invalid(format!("body `{}` cannot be moved into the frame: {e}", b.name)))?;
            *b = moved;
        }
    }
    for (s, m) in out.sketches.iter_mut().zip(&sketch_m) {
        // Only a sketch the frame moves is copied.
        let maps_wire = |w: &solidcraft_sketch::Wire| !crate::is_identity(if w.space == solidcraft_sketch::WireSpace::Component { m } else { &inv });
        if crate::is_identity(m) && !s.sketch.wires.iter().any(maps_wire) {
            continue;
        }
        let s = Arc::make_mut(s);
        if !crate::is_identity(m) {
            s.plane = placed_plane(m, &s.plane)?;
        }
        for wire in &mut s.sketch.wires {
            let matrix = if wire.space == solidcraft_sketch::WireSpace::Component { m } else { &inv };
            map_wire(wire, matrix);
        }
    }
    Ok(Cow::Owned(out))
}

/// `framed` (the model `st` in the frame that `frame` places in the world) with the further
/// placements `names` names (`Block (Part:2)`), where they are in the world. A component's model
/// holds its first placement only, so a sketch projecting a copy's face found no body of that
/// name and took the first placement's (#9957). Only the named ones are placed: placing every
/// body of an assembly for each sketch made sketch edits as slow as the design is big.
pub fn with_placed_copies<'a>(
    doc: &Document,
    st: &ModelState,
    framed: std::borrow::Cow<'a, ModelState>,
    frame: &Mat,
    names: &[&str],
) -> std::borrow::Cow<'a, ModelState> {
    let missing: Vec<&str> = names.iter().copied().filter(|n| !n.is_empty() && framed.body(n).is_none()).collect();
    if missing.is_empty() || doc.occurrences.is_empty() {
        return framed;
    }
    let Some(inv) = crate::mat_inverse(frame) else { return framed };
    let mut by_component: std::collections::HashMap<u64, Vec<&ModelBody>> = std::collections::HashMap::new();
    for (b, owner) in st.bodies.iter().zip(doc.model_body_components(st)) {
        by_component.entry(owner).or_default().push(b);
    }
    let world = crate::WorldNames::new(doc, st);
    let mut placed: Vec<ModelBody> = Vec::new();
    for (comp, path, m) in doc.placements() {
        for b in by_component.get(&comp).into_iter().flatten() {
            let name = world.name(&b.name, comp, &path);
            if !missing.contains(&name.as_str()) || placed.iter().any(|p| p.name == name) {
                continue;
            }
            let bm = doc.model_body_offset(b).map(|o| crate::mat_mul(&m, o)).unwrap_or(m);
            if let Ok(mut copy) = st.body_in_frame(b, &crate::mat_mul(&inv, &bm)) {
                copy.name = name;
                copy.source_name = Some(b.definition_name().to_string());
                placed.push(copy);
            }
        }
    }
    if placed.is_empty() {
        return framed;
    }
    let mut out = framed.into_owned();
    out.bodies.extend(placed);
    std::borrow::Cow::Owned(out)
}

/// The model placed in the world: each component's bodies, sketches, threads and construction
/// geometry through every occurrence of it (bodies of the first placement keep their names;
/// further instances get the occurrence path in brackets). Without components this is the model
/// itself.
pub fn world_state(doc: &Document, st: &ModelState) -> Result<ModelState> {
    world_state_reusing(doc, st, &mut PlacedBodies::default())
}

/// Bodies a [`world_state_reusing`] call placed away from their definition (a moved
/// component's bodies), by their definition and placement: the next call reuses each one whose
/// definition and placement are unchanged instead of transforming it again, and so keeps its
/// display mesh. Without this every change anywhere in a design transformed and meshed every
/// body of every moved component again.
#[derive(Debug, Default)]
pub struct PlacedBodies {
    by_source: std::collections::HashMap<(usize, [u64; 16]), (ModelBody, ModelBody)>,
}

impl PlacedBodies {
    fn reused(&self, b: &ModelBody, m: &crate::Mat) -> Option<&ModelBody> {
        self.by_source
            .get(&placed_key(b, m))
            .filter(|(source, _)| {
                Arc::ptr_eq(&source.mesh, &b.mesh)
                    && source.body.same(&b.body)
                    && source.body.color() == b.body.color()
                    && source.body.paint() == b.body.paint()
                    && match (source.body.split_lines(), b.body.split_lines()) {
                        (Some(x), Some(y)) => Arc::ptr_eq(x, y),
                        (None, None) => true,
                        _ => false,
                    }
            })
            .map(|(_, placed)| placed)
    }

    /// The body `b` placed by `m`, from an earlier call when `b` is the same body as then; an
    /// error when it cannot be placed.
    fn placed(&self, b: &ModelBody, m: &crate::Mat, name: String) -> Result<ModelBody> {
        if let Some(placed) = self.taken(b, m, &name) {
            return Ok(placed);
        }
        // The identity places the body as it is, without a copy through the kernel.
        let body = if crate::is_identity(m) {
            b.body.clone()
        } else {
            kernel::place_matrix(&b.body, *m).map_err(|e| DocError::Invalid(format!("body `{}` cannot be placed: {e}", b.name)))?
        };
        let mut fresh = ModelBody::new(name, body, b.feature);
        fresh.mesh_placement = placed_mesh::PlacedMesh::new(b, m).map(Arc::new);
        Ok(fresh)
    }

    /// The body `b` placed by `m` as an earlier call placed it, when `b` is the same body as then.
    fn taken(&self, b: &ModelBody, m: &crate::Mat, name: &str) -> Option<ModelBody> {
        self.reused(b, m).map(|placed| {
            let mut fresh = ModelBody::new(name.to_string(), placed.body.clone(), b.feature);
            fresh.mesh = placed.mesh.clone();
            fresh.bounds = placed.bounds.clone();
            fresh.measured = placed.measured.clone();
            fresh.joint_faces = placed.joint_faces.clone();
            fresh.faces = placed.faces.clone();
            fresh.mesh_placement = placed.mesh_placement.clone();
            fresh.plan = placed.plan.clone();
            fresh.gpu_plan = placed.gpu_plan.clone();
            fresh.plan_mesh = placed.plan_mesh.clone();
            fresh
        })
    }
}

/// Bodies [`state_in_frame_at`] moved into a feature's frame, by source body and matrix: a
/// sketch on a face of a turned component is evaluated in that component's frame, and without
/// this every edit of it moved (and so meshed again) every body placed differently from it.
/// Held by the [`Model`] across evaluations; each `evaluate` that moves a body starts a new
/// generation, and an entry neither it nor the last such evaluation before it used is dropped,
/// so the cache holds only the placements in use. An evaluation that moves nothing (every
/// result reused, or taken from the cache: an undo, the refreshes a command makes after its
/// edit) leaves the generations alone: counting it, two of them in a row dropped every moved
/// body, and the next edit of the sketch moved and meshed them all again.
#[derive(Debug, Default)]
pub(crate) struct FrameBodies {
    now: PlacedBodies,
    before: PlacedBodies,
    /// An evaluation began and has not used the cache yet: its first use starts the generation.
    pending: bool,
}

impl FrameBodies {
    /// At most this many bodies are kept per generation.
    const CAP: usize = 4096;

    /// An evaluation begins; its generation starts when it first moves a body.
    pub(crate) fn next_generation(&mut self) {
        self.pending = true;
    }

    fn begin(&mut self) {
        if std::mem::take(&mut self.pending) {
            self.before = std::mem::take(&mut self.now);
        }
    }

    /// The moved body for `b` under `m`, when a call in this or the previous generation moved
    /// the same body by the same matrix.
    fn reused(&mut self, b: &ModelBody, m: &crate::Mat) -> Option<ModelBody> {
        self.begin();
        if let Some(placed) = self.now.reused(b, m) {
            return Some(placed.clone());
        }
        let placed = self.before.reused(b, m)?.clone();
        let key = placed_key(b, m);
        if let Some(entry) = self.before.by_source.remove(&key)
            && self.now.by_source.len() < Self::CAP
        {
            self.now.by_source.insert(key, entry);
        }
        Some(placed)
    }

    fn keep(&mut self, b: &ModelBody, m: &crate::Mat, moved: &ModelBody) {
        self.begin();
        if self.now.by_source.len() < Self::CAP {
            self.now.by_source.insert(placed_key(b, m), (b.clone(), moved.clone()));
        }
    }
}

/// `b` in a feature's frame: `placed` (its geometry moved by `m`, sharing the moved body's
/// mesh and display plan), carrying `b`'s ownership and naming.
fn in_frame(b: &ModelBody, m: &crate::Mat, placed: &ModelBody) -> ModelBody {
    let mut moved = ModelBody::new(b.name.clone(), placed.body.clone(), b.feature);
    moved.mesh = placed.mesh.clone();
    moved.plan = placed.plan.clone();
    moved.gpu_plan = placed.gpu_plan.clone();
    moved.plan_mesh = placed.plan_mesh.clone();
    moved.component = b.component;
    moved.transferred = b.transferred;
    moved.source_name = b.source_name.clone();
    moved.source_transform = crate::mat_mul(m, &b.source_transform);
    moved.imported = b.imported.clone();
    // Moving a body keeps its faces (and their order): it keeps their names, and each face's
    // sketch frame moves with it.
    moved.names = crate::naming::NameCell::same(b);
    let placed_frame = |p: &Option<Plane>| p.as_ref().and_then(|p| placed_plane(m, p).ok());
    moved.sketch_frames = b.sketch_frames.as_ref().map(|frames| Arc::new(frames.iter().map(placed_frame).collect()));
    moved
}

fn placed_key(b: &ModelBody, m: &crate::Mat) -> (usize, [u64; 16]) {
    let mut bits = [0u64; 16];
    for (slot, v) in bits.iter_mut().zip(m.iter().flatten()) {
        *slot = v.to_bits();
    }
    (Arc::as_ptr(&b.mesh) as usize, bits)
}

/// Whether a world state shows `b`, placed by `m`, apart from the model's body. A mirrored copy
/// always is: it is drawn from its source's opposite hand, and the model keeps its own triangles.
fn placed_apart(b: &ModelBody, m: &crate::Mat) -> bool {
    !crate::is_identity(m) || b.mirrored_mesh.is_some()
}

/// [`world_state`], reusing the bodies `placed` holds from an earlier call and leaving it holding
/// this call's.
pub fn world_state_reusing(doc: &Document, st: &ModelState, placed_before: &mut PlacedBodies) -> Result<ModelState> {
    world_state_moved_reusing(doc, st, &std::collections::BTreeMap::new(), placed_before)
}

/// [`world_state_reusing`] with the occurrences in `moves` placed by those transforms (in their
/// parents) instead of their own ([`Document::placements_moved`]).
pub fn world_state_moved_reusing(
    doc: &Document,
    st: &ModelState,
    moves: &std::collections::BTreeMap<u64, crate::Mat>,
    placed_before: &mut PlacedBodies,
) -> Result<ModelState> {
    if doc.occurrences.is_empty() {
        return Ok(st.clone());
    }
    let mut placed_now = PlacedBodies::default();
    // Looked up once: every sketch, construction and tap asked the timeline, and every placement
    // went through all bodies, each time searching (a large assembly paid this every command).
    let feature_components: std::collections::HashMap<u64, u64> = doc.features.iter().rev().map(|f| (f.id, f.component)).collect();
    let comp_of_feature = |id: u64| feature_components.get(&id).copied().unwrap_or(0);
    let first_path = |comp: u64| -> Vec<u64> {
        let mut path = Vec::new();
        let mut c = comp;
        for _ in 0..1000 {
            if c == 0 {
                break;
            }
            let Some(o) = doc.occurrence_of(c) else { break };
            path.insert(0, o.id);
            c = o.parent;
        }
        path
    };
    let mut out = ModelState { body_counter: st.body_counter, ..Default::default() };
    let placements = doc.placements_moved(moves);
    // A body moved in from another component keeps its place (its offset).
    let body_matrix = |b: &ModelBody, m: &crate::Mat| doc.model_body_offset(b).map(|o| crate::mat_mul(m, o)).unwrap_or(*m);
    // Every body placed elsewhere than its definition, placed on all cores: a large assembly
    // places hundreds of parts, each a B-rep transform.
    let mut jobs = Vec::new();
    let owners = doc.model_body_components(st);
    let names = crate::WorldNames::new(doc, st);
    let mut by_component: std::collections::HashMap<u64, Vec<&ModelBody>> = std::collections::HashMap::new();
    for (b, owner) in st.bodies.iter().zip(&owners) {
        by_component.entry(*owner).or_default().push(b);
    }
    let bodies_of = |comp: u64| by_component.get(&comp).into_iter().flatten().copied();
    // Each tap's component and the offset of the body it is on (the first body of that name).
    let mut first_body: std::collections::HashMap<&str, (&ModelBody, u64)> = std::collections::HashMap::new();
    for (b, owner) in st.bodies.iter().zip(&owners).rev() {
        first_body.insert(b.name.as_str(), (b, *owner));
    }
    let tap_frames: Vec<(u64, Option<&crate::Mat>)> = st
        .threads
        .iter()
        .map(|t| match first_body.get(t.body.as_str()) {
            Some((body, owner)) => (*owner, doc.model_body_offset(body)),
            None => (comp_of_feature(t.feature), None),
        })
        .collect();
    for (comp, path, m) in &placements {
        for b in bodies_of(*comp) {
            let bm = body_matrix(b, m);
            if placed_apart(b, &bm) {
                jobs.push((b, bm, names.name(&b.name, *comp, path)));
            }
        }
    }
    // Bodies placed as before are taken here: starting threads only to copy them cost every
    // command in a design with placed components more than the copies.
    let reused: Vec<_> = jobs.iter().map(|(b, bm, name)| placed_before.taken(b, bm, name)).collect();
    let fresh: Vec<_> = jobs.iter().zip(&reused).filter(|(_, r)| r.is_none()).map(|(job, _)| job).collect();
    let mut placed_fresh = kernel::map_on_threads(&fresh, 16, |(b, bm, name)| placed_before.placed(b, bm, name.clone())).into_iter();
    let mut moved = reused.into_iter().map(|r| match r {
        Some(placed) => Some(Ok(placed)),
        None => placed_fresh.next().flatten(),
    });
    for (comp, path, m) in placements {
        let primary = path == first_path(comp);
        let ident = crate::is_identity(&m);
        for b in bodies_of(comp) {
            let bm = body_matrix(b, &m);
            // Identical geometry also shares its display mesh cell with the definition.
            // A changed placement keeps a distinct mesh, at that placement's coordinates.
            let mut placed = if !placed_apart(b, &bm) {
                let mut placed = b.clone();
                placed.name = names.name(&b.name, comp, &path);
                placed
            } else {
                // The same order as the jobs: this body's, made above.
                let placed = match moved.next() {
                    Some(Some(placed)) => placed?,
                    Some(None) => return Err(DocError::Invalid(format!("body `{}` could not be placed (its worker failed)", b.name))),
                    None => return Err(DocError::Invalid(format!("body `{}` has no placement", b.name))),
                };
                placed_now.by_source.insert(placed_key(b, &bm), (b.clone(), placed.clone()));
                placed
            };
            // World states keep their previous display-only evaluation-key semantics.
            placed.key = 0;
            // With the model body's own frame, so its place is told apart from the placement's.
            placed.source_key = b.geometry_key().unwrap_or(0);
            placed.source_name = Some(b.definition_name().to_string());
            placed.source_transform = crate::mat_mul(&bm, &b.source_transform);
            placed.component = comp;
            placed.transferred = b.transferred;
            placed.imported = b.imported.clone();
            // Placement preserves topology: face ownership remains that of the
            // source model, including modifiers on component instances.
            placed.names = crate::naming::NameCell::same(b);
            out.bodies.push(placed);
        }
        if primary {
            for s in st.sketches.iter().filter(|s| comp_of_feature(s.feature) == comp) {
                if ident {
                    // In place: shared as it is.
                    out.sketches.push(s.clone());
                    continue;
                }
                let mut s2 = SolvedSketch::clone(s);
                s2.plane = placed_plane(&m, &s.plane)?;
                for wire in &mut s2.sketch.wires {
                    if wire.space == solidcraft_sketch::WireSpace::Component {
                        map_wire(wire, &m);
                    }
                }
                out.sketches.push(Arc::new(s2));
            }
            // The component's construction planes, axes and points where it is placed: once the
            // design has a component the root's constructions are built in this state
            // (`construction_state`), and one built on another finds it here by name.
            for c in st.construct.iter().filter(|c| comp_of_feature(c.feature) == comp) {
                let geom = if ident { c.geom.clone() } else { c.geom.placed(&m) };
                out.construct.push(crate::construct::Construct { geom, ..c.clone() });
            }
        }
        // Unlike editable sketch guides, cosmetic surfaces belong to every occurrence.
        for (t, (_, offset)) in st.threads.iter().zip(&tap_frames).filter(|(_, (owner, _))| *owner == comp) {
            let mut t2 = t.clone();
            t2.body = names.name(&t.body, comp, &path);
            let frame = offset.map(|offset| crate::mat_mul(&m, offset)).unwrap_or(m);
            t2.axis_point = crate::apply_point(&frame, t.axis_point);
            t2.axis = crate::apply_vector(&frame, *t.axis).unit().ok_or_else(|| DocError::Invalid("a thread's axis cannot be placed".into()))?;
            out.threads.push(t2);
        }
    }
    *placed_before = placed_now;
    Ok(out)
}

/// `plane` for a feature of component `comp`, with each construction plane of another component
/// (under any offset or flip of it) given as the plane where that component puts it, in `comp`'s
/// frame (#11734). A construction plane is kept in its own component's frame, as the axes are
/// (`construction_axis_in`), so a plane read as it is would fall in the wrong place for a feature
/// of a component placed differently. A captured construction plane is resolved directly in its
/// saved frame by `resolve_plane_in`, keeping assembly motion out of the sketch's definition.
fn construction_planes_in_frame<'a>(doc: &Document, st: &ModelState, comp: u64, plane: &'a PlaneRef) -> Result<std::borrow::Cow<'a, PlaneRef>> {
    use std::borrow::Cow;
    // The component of a feature, read from the timeline without `Document::feature`'s search counter.
    fn owner_of(doc: &Document, id: u64) -> Option<u64> {
        doc.features.iter().find(|f| f.id == id).map(|f| f.component)
    }
    fn foreign(doc: &Document, comp: u64, plane: &PlaneRef) -> bool {
        match plane {
            PlaneRef::Construction { feature, placement } => placement.is_none() && owner_of(doc, feature.0).is_some_and(|owner| owner != comp),
            PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::ThroughPoint { base, .. } => {
                foreign(doc, comp, base)
            }
            _ => false,
        }
    }
    fn map(doc: &Document, st: &ModelState, comp: u64, plane: &mut PlaneRef) -> Result<()> {
        match plane {
            PlaneRef::Construction { feature, placement } => {
                if placement.is_some() {
                    return Ok(());
                }
                let owner = owner_of(doc, feature.0).unwrap_or(comp);
                if owner == comp {
                    return Ok(());
                }
                if let Some(crate::construct::ConstructGeom::Plane(pl)) = st.construct.iter().find(|c| c.feature == feature.0).map(|c| &c.geom) {
                    let inverse = crate::mat_inverse(&doc.component_transform(comp))
                        .ok_or_else(|| DocError::Invalid("the feature's component frame is invalid".into()))?;
                    let m = crate::mat_mul(&inverse, &doc.component_transform(owner));
                    *plane = PlaneRef::Custom { plane: placed_plane(&m, pl)? };
                }
            }
            PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::ThroughPoint { base, .. } => {
                map(doc, st, comp, base)?
            }
            _ => {}
        }
        Ok(())
    }
    if doc.occurrences.is_empty() || !foreign(doc, comp, plane) {
        return Ok(Cow::Borrowed(plane));
    }
    let mut mapped = plane.clone();
    map(doc, st, comp, &mut mapped)?;
    Ok(Cow::Owned(mapped))
}

/// `plane` moved by the placement `m`; an error when `m` does not keep it a plane (a placement
/// that is not a rigid motion, which [`Document`] does not take).
pub(crate) fn placed_plane(m: &crate::Mat, plane: &Plane) -> Result<Plane> {
    Plane::new(crate::apply_point(m, plane.origin), crate::apply_vector(m, plane.x), crate::apply_vector(m, plane.y))
        .ok_or_else(|| DocError::Invalid("a sketch plane cannot be placed: the placement is not a rigid motion".into()))
}

/// The occurrence path that places each body of the [`world_state`] of `doc` and `st`, in the
/// order of its bodies (a body of the root's own: an empty path). Bodies under the same moved
/// occurrence move together: a joint drag draws them as one rigid layer (#6325) without placing
/// them again.
pub fn world_body_paths(doc: &Document, st: &ModelState) -> Vec<Vec<u64>> {
    if doc.occurrences.is_empty() {
        return vec![Vec::new(); st.bodies.len()];
    }
    // How many bodies each component owns, counted once: a placement then costs its own bodies,
    // not a pass over every body of the design.
    let mut owned: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for owner in doc.model_body_components(st) {
        *owned.entry(owner).or_default() += 1;
    }
    let mut out = Vec::with_capacity(st.bodies.len());
    // The same walk as `world_state_moved_reusing`: placements in order, each its component's bodies.
    for (comp, path, _) in doc.placements() {
        out.extend((0..owned.get(&comp).copied().unwrap_or(0)).map(|_| path.clone()));
    }
    out
}

#[cfg(test)]
mod display_preparation_tests {
    use super::*;

    fn state() -> ModelState {
        let body = kernel::box_solid(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0)).unwrap();
        ModelState { bodies: vec![ModelBody::new("Box".into(), body, 1)], ..Default::default() }
    }

    #[test]
    fn prepared_display_reads_all_three_caches_without_cold_geometry_or_quality_changes() {
        use crate::display_quality::{DisplayQuality, current};
        let state = state();
        let body = &state.bodies[0];
        let quality = DisplayQuality::Fine;
        let index = quality.index();
        let current = current();
        let tolerance = body.display_tol();
        assert!(body.mesh.get().is_none() && body.plan[index].get().is_none() && body.plan_mesh[index].get().is_none());
        state.warm_display_at(quality, || true);
        // Inspect the caches before invoking any consumer that could initialize them.
        let reference = body.mesh.get().unwrap();
        let plan = body.plan[index].get().unwrap().as_ref().unwrap();
        let display = body.plan_mesh[index].get().unwrap().as_ref().unwrap();
        assert!(Arc::ptr_eq(reference, &body.mesh()));
        assert!(Arc::ptr_eq(plan, &body.display_plan_at(quality).unwrap()));
        assert!(Arc::ptr_eq(display, &body.plan_mesh_at(quality).unwrap()));
        assert_eq!(crate::display_quality::current(), current);
        assert_eq!(body.display_tol(), tolerance);
        for other in DisplayQuality::ALL.into_iter().filter(|other| *other != quality) {
            assert!(body.plan[other.index()].get().is_none() && body.plan_mesh[other.index()].get().is_none());
        }
    }

    #[test]
    fn cancelled_display_preparation_leaves_cold_caches_untouched() {
        let state = state();
        state.warm_display_at(crate::display_quality::DisplayQuality::Normal, || false);
        let body = &state.bodies[0];
        assert!(body.mesh.get().is_none());
        assert!(body.plan.iter().all(|slot| slot.get().is_none()));
        assert!(body.plan_mesh.iter().all(|slot| slot.get().is_none()));
    }
}

#[cfg(test)]
mod section_tests {
    use super::*;

    /// A 10 mm cube from the origin, keyed as an evaluated body is.
    fn cube() -> ModelBody {
        let mut cube = ModelBody::new("Cube".into(), kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap(), 1);
        cube.key = 7;
        cube
    }

    /// The plane `z` up from the origin, its normal up.
    fn at(z: f64) -> Plane {
        Plane { origin: Vec3::new(0.0, 0.0, z), x: Vec3::X, y: Vec3::Y }
    }

    /// The lowest and highest z of `body`'s exact geometry.
    fn heights(body: &ModelBody) -> (f64, f64) {
        let b = body.body.exact_bounds().unwrap();
        (b.min.z, b.max.z)
    }

    /// A section keeps the side its plane's normal points to, named as the body and with no key of
    /// its own. Every copy of the body knows it without cutting again; another plane cuts again, a
    /// plane past the body keeps nothing, and a body with no key cuts each time.
    #[test]
    fn a_bodys_section_is_remembered_by_its_copies() {
        let cube = cube();
        let upper = cube.section(&at(4.0)).unwrap().unwrap();
        let (low, high) = heights(&upper);
        assert!((low - 4.0).abs() < 1e-9 && (high - 10.0).abs() < 1e-9, "{low} {high}");
        assert_eq!((upper.name.as_str(), upper.geometry_key()), ("Cube", None));
        assert!(cube.clone().section(&at(4.0)).unwrap().unwrap().body.same(&upper.body));
        let higher = cube.section(&at(6.0)).unwrap().unwrap();
        assert!(!higher.body.same(&upper.body) && (heights(&higher).0 - 6.0).abs() < 1e-9);
        assert!(cube.section(&at(10.5)).unwrap().is_none());
        let unkeyed = ModelBody::new("Cube".into(), cube.body.clone(), 1);
        let first = unkeyed.section(&at(4.0)).unwrap().unwrap();
        assert!(!unkeyed.section(&at(4.0)).unwrap().unwrap().body.same(&first.body));
    }
}

#[cfg(test)]
mod world_placement_cache_tests {
    use super::*;

    #[test]
    fn placed_real_feature_retains_names_from_its_original_feature_context() {
        let mut doc = Document::new("authored names");
        let component = doc.add_component(Some("Part"), 0).unwrap();
        let feature = doc
            .add_feature(
                FeatureKind::Box {
                    corner: crate::point_expr(Vec3::ZERO),
                    length: "2 mm".into(),
                    width: "3 mm".into(),
                    height: "4 mm".into(),
                    operation: crate::Operation::NewBody,
                    centred: false,
                },
                Some("AuthoredBox"),
            )
            .unwrap();
        doc.feature_mut(feature).unwrap().component = component;
        let matrix = crate::rigid(Vec3::new(15., -2., 8.), Vec3::ZERO, Vec3::X, 0.7);
        doc.occurrences.iter_mut().find(|o| o.component == component).unwrap().transform = matrix;
        let mut model = Model::default();
        model.evaluate(&doc);
        let state = model.state();
        let source = &state.bodies[0];
        assert!(source.names.origin.is_some());
        assert!(source.mesh.get().is_none());
        let world = world_state(&doc, &state).unwrap();
        let names = crate::naming::face_names(&world.bodies[0]);
        assert!(Arc::ptr_eq(&names, &crate::naming::face_names(source)));
        assert!(names.iter().all(|name| name.starts_with(&format!("F{feature}:"))));
        let local = source.mesh();
        let placed = world.bodies[0].mesh();
        assert_eq!(local.tri_face, placed.tri_face);
        let mapped: Vec<_> = local.positions.iter().map(|p| crate::apply_point(&matrix, *p)).collect();
        assert!(mapped.iter().all(|p| placed.positions.iter().any(|q| p.dist(*q) < 1e-8)));
        assert!(placed.positions.iter().all(|p| mapped.iter().any(|q| p.dist(*q) < 1e-8)));
    }

    /// A body asked to be called a name a linked component's body has is named otherwise; a
    /// later feature of its component that targets the name is refused with both bodies listed,
    /// not run on the other component's body (#12141).
    #[test]
    fn a_target_name_taken_by_another_component_is_refused_as_ambiguous() {
        let mut doc = Document::new("names");
        let linked = doc.add_component(Some("Encoder"), 0).unwrap();
        let local = doc.add_component(Some("Assembly"), 0).unwrap();
        let mut boxes = |comp: u64, operation: Operation, corner: Vec3| {
            let id = doc
                .add_feature(
                    FeatureKind::Box {
                        corner: crate::point_expr(corner),
                        length: "10".into(),
                        width: "10".into(),
                        height: "2".into(),
                        operation,
                        centred: false,
                    },
                    None,
                )
                .unwrap();
            let f = doc.feature_mut(id).unwrap();
            f.component = comp;
            id
        };
        let a = boxes(linked, Operation::NewBody, Vec3::new(100., 0., 0.));
        let b = boxes(local, Operation::NewBody, Vec3::ZERO);
        let cut = boxes(local, Operation::Cut, Vec3::new(2., 2., -1.));
        doc.feature_mut(a).unwrap().body_names = vec!["AS5047P_PCB".into()];
        doc.feature_mut(b).unwrap().body_names = vec!["AS5047P_PCB".into()];
        if let FeatureKind::Box { length, width, .. } = &mut doc.feature_mut(cut).unwrap().kind {
            *length = "2".into();
            *width = "2".into();
        }
        doc.feature_mut(cut).unwrap().participants = vec!["AS5047P_PCB".into()];
        let mut model = Model::new();
        model.evaluate(&doc);
        let st = model.state();
        let own = st.bodies.iter().find(|x| x.component == local).unwrap().name.clone();
        assert_ne!(own, "AS5047P_PCB", "names stay unique");
        let err = model.result(cut).and_then(|r| r.error.clone()).expect("the cut is refused");
        assert!(err.contains("ambiguous") && err.contains("`Encoder`") && err.contains(&format!("`{own}`")), "{err}");
        // The linked body is left alone.
        let linked_body = st.body("AS5047P_PCB").unwrap();
        assert_eq!(linked_body.component, linked);

        // Targeting the local body by the name it has works.
        doc.feature_mut(cut).unwrap().participants = vec![own.clone()];
        let mut model = Model::new();
        model.evaluate(&doc);
        assert!(model.result(cut).and_then(|r| r.error.clone()).is_none());
    }

    /// An Import's skipped solids make no body: its bodies take the stored names left, so one
    /// built from slot B was asked for B, not for the skipped slot's A (#12141).
    #[test]
    fn an_imports_body_is_asked_for_its_own_slots_name() {
        let mut doc = Document::new("names");
        let id = doc
            .add_feature(FeatureKind::Import { file: "x.step".into(), step: "".into(), components: Vec::new(), skip: vec![0], records: None }, None)
            .unwrap();
        doc.feature_mut(id).unwrap().body_names = vec!["A".into(), "B".into()];
        let mut state = ModelState::default();
        state.bodies.push(ModelBody::new("B (2)".into(), kernel::box_solid(Vec3::ZERO, Vec3::new(1., 1., 1.)).unwrap(), id));
        assert_eq!(asked_names(&doc, &state).next().unwrap().1, Some("B"));
        doc.feature_mut(id).unwrap().kind =
            FeatureKind::Import { file: "x.step".into(), step: "".into(), components: Vec::new(), skip: Vec::new(), records: None };
        assert_eq!(asked_names(&doc, &state).next().unwrap().1, Some("A"));
    }

    /// `ModelBody::body` is public: a body replaced in place keeps its geometry key, and a joint
    /// origin kept for the old geometry must not be answered for the new (#17074).
    #[test]
    fn a_joint_origin_follows_a_body_replaced_in_place() {
        use crate::joints::{JointOrigin, Snap};
        let doc = Document::new("replaced keyed body");
        let mut state = ModelState::default();
        state.bodies.push(ModelBody::new("Pin".into(), kernel::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 10.0).unwrap(), 0));
        state.bodies[0].key = 77;
        assert!(state.bodies[0].geometry_key().is_some());
        let origin = JointOrigin::new(0, Snap::FaceCenter { pick: Vec3::new(0.0, 0.0, 10.0), face: None, x: None });
        let first = crate::joints::resolve_origin(&doc, &state, &origin).unwrap();
        assert!((first[3][2] - 10.0).abs() < 1e-9);
        assert_eq!(state.origin_resolutions(), 1);
        crate::joints::resolve_origin(&doc, &state, &origin).unwrap();
        assert_eq!(state.origin_resolutions(), 1, "the same bodies keep their frame");
        state.bodies[0].body = kernel::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 12.0).unwrap();
        let changed = crate::joints::resolve_origin(&doc, &state, &origin).unwrap();
        assert!((changed[3][2] - 12.0).abs() < 1e-9, "{changed:?}");
        assert_eq!(state.origin_resolutions(), 2);
    }

    #[test]
    fn identity_shares_mesh_but_rotated_instances_keep_distinct_physical_coordinates_and_source_names() {
        let mut doc = Document::new("placements");
        let component = doc.add_component(Some("Part"), 0).unwrap();
        doc.body_components.insert("Block".into(), component);
        let matrix = crate::rigid(Vec3::new(10., 20., 0.), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2);
        let second = doc.add_occurrence(component, 0, matrix).unwrap();
        let mut state = ModelState::default();
        state.bodies.push(ModelBody::new("Block".into(), kernel::box_solid(Vec3::ZERO, Vec3::new(2., 3., 4.)).unwrap(), 1));
        state.bodies[0].key = 1234;
        let source = &state.bodies[0];
        let world = world_state(&doc, &state).unwrap();
        assert_eq!(world.bodies.len(), 2);
        let first = world.body("Block").unwrap();
        let second_name = doc.world_body_name(&state, "Block", component, &[second]);
        let placed = world.body(&second_name).unwrap();
        assert!(Arc::ptr_eq(&source.mesh, &first.mesh), "unchanged geometry must retain the shared display mesh cell");
        assert_eq!(first.key, 0);
        assert_eq!(placed.key, 0);
        // Each placement has its own geometry identity, for work measured on it.
        assert!(first.geometry_key().is_some() && first.geometry_key() != placed.geometry_key());
        assert!(!Arc::ptr_eq(&source.mesh, &placed.mesh), "a transformed mesh cannot be reused without mapping coordinates");
        assert_eq!(first.source_name.as_deref(), Some("Block"));
        assert_eq!(placed.source_name.as_deref(), Some("Block"));
        assert_eq!(placed.source_transform, matrix);
        assert_eq!(doc.model_body_component(placed), component);
        let bounds = placed.mesh().bounds();
        assert!(bounds.min.dist(Vec3::new(7., 20., 0.)) < 1e-9);
        assert!(bounds.max.dist(Vec3::new(10., 22., 4.)) < 1e-9);
        let names = crate::naming::face_names(source);
        assert!(Arc::ptr_eq(&names, &crate::naming::face_names(first)));
        assert!(Arc::ptr_eq(&names, &crate::naming::face_names(placed)));
        let original_mesh = source.mesh();
        assert!(Arc::ptr_eq(&original_mesh, &first.mesh()));
        let again = world_state(&doc, &state).unwrap();
        assert!(Arc::ptr_eq(&original_mesh, &again.body("Block").unwrap().mesh()));
        assert!(Arc::ptr_eq(&names, &crate::naming::face_names(again.body(&second_name).unwrap())));
    }

    /// A dragged component's bodies are drawn from the meshes and plans made where the drag found
    /// them, moved: each is the same solid moved rigidly, the others stand where they were, and a
    /// body whose geometry changed is no such move.
    #[test]
    fn a_moved_placement_is_drawn_moved_from_where_it_stood() {
        // A placement moved, and the part moved off where it is defined.
        for from_definition in [false, true] {
            let mut doc = Document::new("placements");
            let component = doc.add_component(Some("Part"), 0).unwrap();
            doc.body_components.insert("Block".into(), component);
            let start = if from_definition { crate::IDENTITY } else { crate::rigid(Vec3::new(10., 20., 0.), Vec3::ZERO, Vec3::Z, 0.4) };
            let second = doc.add_occurrence(component, 0, start).unwrap();
            let mut state = ModelState::default();
            state.bodies.push(ModelBody::new("Block".into(), kernel::box_solid(Vec3::ZERO, Vec3::new(2., 3., 4.)).unwrap(), 1));
            let before = world_state(&doc, &state).unwrap();
            let delta = crate::rigid(Vec3::new(4., -7., 1.5), Vec3::new(1., 1., 0.), Vec3::new(0., 1., 1.), 0.6);
            let moved = doc.occurrences.iter_mut().find(|o| o.id == second).unwrap();
            moved.transform = crate::mat_mul(&delta, &moved.transform);
            let after = world_state(&doc, &state).unwrap();
            assert_eq!(after.bodies.len(), 2);
            let mut moves = 0;
            for now in &after.bodies {
                let was = before.body(&now.name).unwrap();
                let d = now.display_moved_from(was).unwrap();
                if d == crate::IDENTITY {
                    assert!(Arc::ptr_eq(&now.mesh(), &was.mesh()), "{}: unchanged, the same mesh", now.name);
                    continue;
                }
                moves += 1;
                assert!(d.iter().flatten().zip(delta.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-9), "{}: {d:?}", now.name);
                // `was`'s mesh drawn moved is `now`'s.
                for (p, q) in was.mesh().positions.iter().zip(&now.mesh().positions) {
                    assert!(crate::apply_point(&d, *p).dist(*q) < 1e-9);
                }
            }
            assert_eq!(moves, 1, "from its definition: {from_definition}");
            // Another body, or other geometry: no move draws it.
            assert!(after.bodies[0].display_moved_from(&before.bodies[1]).is_none());
            let other = ModelBody::new(before.bodies[0].name.clone(), kernel::box_solid(Vec3::ZERO, Vec3::new(1., 1., 1.)).unwrap(), 1);
            assert!(other.display_moved_from(&before.bodies[0]).is_none());
        }
    }
}

#[cfg(test)]
mod chain_tests {
    use super::*;

    /// The top face of a box with one rounded top edge chains to the round and on to the side
    /// it rolls into; the other sides meet at sharp edges and stay out.
    #[test]
    fn tangent_chain_follows_smooth_edges() {
        let mut doc = Document::new("T");
        doc.add_feature(
            FeatureKind::Box {
                corner: crate::point_expr(Vec3::ZERO),
                length: "40".into(),
                width: "30".into(),
                height: "20".into(),
                operation: Operation::NewBody,
                centred: false,
            },
            None,
        )
        .unwrap();
        doc.add_feature(
            FeatureKind::Fillet {
                edges: vec![crate::Position(Vec3::new(20.0, 0.0, 20.0))],
                radius: "4".into(),
                body: None,
                style: crate::FilletStyle::Constant,
                fit: Default::default(),
                corner: Default::default(),
                setbacks: Vec::new(),
                turn: Default::default(),
                tangent_chain: true,
                edge_radii: Vec::new(),
            },
            None,
        )
        .unwrap();
        let mut m = Model::new();
        m.evaluate(&doc);
        let st = m.state();
        let b = &st.bodies[0];
        let pts = tangent_face_chain(b, &[Vec3::new(20.0, 15.0, 20.0)]);
        let faces: std::collections::BTreeSet<usize> = pts.iter().map(|(index, _)| *index).collect();
        assert_eq!(faces.len(), 3, "{faces:?}");
        assert_eq!(pts.len(), faces.len());
        for (index, point) in &pts {
            assert_eq!(crate::appearance::face_index_at(b, *point), Some(*index));
        }
        assert_eq!(tangent_face_chain(b, &[Vec3::new(20.0, 15.0, 20.0); 2]), pts);
    }

    /// A boss on a stepped plate: the boss's concave side edge at x 20 (z 20…40) runs on in a
    /// straight line as the step's convex edge (z 0…20). A Fillet dialog's tangent chain from
    /// the boss edge is that edge alone, as the feature's exact chain is: a round cannot pass
    /// from one side of the faces to the other. The display mesh's walk took both.
    #[test]
    fn a_blend_chain_ends_where_a_concave_edge_runs_on_as_a_convex_one() {
        let mut doc = Document::new("T");
        let block = |corner: Vec3, [l, w, h]: [&str; 3], operation| FeatureKind::Box {
            corner: crate::point_expr(corner),
            length: l.into(),
            width: w.into(),
            height: h.into(),
            operation,
            centred: false,
        };
        doc.add_feature(block(Vec3::new(0.0, 0.0, 20.0), ["40", "20", "20"], Operation::NewBody), None).unwrap();
        doc.add_feature(block(Vec3::new(20.0, 0.0, 0.0), ["20", "20", "20"], Operation::Join), None).unwrap();
        doc.add_feature(block(Vec3::new(10.0, -10.0, 20.0), ["10", "10", "20"], Operation::Join), None).unwrap();
        let mut m = Model::new();
        m.evaluate(&doc);
        let st = m.state();
        let b = &st.bodies[0];
        // The edge the kernel takes for a pick, exactly: its index is the display edge's.
        let picked = |p: Vec3| kernel::blend_edges(&b.body, &[p]).unwrap()[0];
        let boss = picked(Vec3::new(20.0, 0.0, 30.0));
        let step = picked(Vec3::new(20.0, 0.0, 10.0));
        assert_ne!(boss, step);
        assert!(b.mesh().tangent_chain(boss, solidcraft_geom::TANGENT_CHAIN).contains(&step), "the mesh walk took the step");
        assert_eq!(b.blend_chain(boss, Vec3::new(20.0, 0.0, 30.0)), vec![boss]);
    }

    /// When the kernel cannot give the points along the edges a blend's picks run on into (here
    /// a mesh body, which has no tangent chain), the note says the check could not run and why,
    /// instead of saying nothing, which reads as "no edge was left sharp" (#19246).
    #[test]
    fn a_left_sharp_check_that_cannot_run_says_so() {
        let mesh = kernel::mesh_body(&[Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z], &[[0, 1, 2], [0, 1, 3], [1, 2, 3], [0, 2, 3]]).unwrap();
        let beyond = chain_beyond_picks(&mesh, &[Vec3::new(0.5, 0.0, 0.0)]);
        let Err(e) = &beyond else { panic!("a mesh body has no tangent chain: {beyond:?}") };
        let note = left_sharp_note(&mesh, &beyond, 1, &fillet(Vec3::new(0.5, 0.0, 0.0), "0.1", false)).expect("a note");
        assert_eq!(note, format!("could not check whether the picked edge runs on smoothly into edges this fillet left sharp ({e})"));
        // A check that ran and found nothing beyond the picks still says nothing.
        assert_eq!(left_sharp_note(&mesh, &Ok(Vec::new()), 1, &fillet(Vec3::new(0.5, 0.0, 0.0), "0.1", false)), None);
    }

    fn fillet(at: Vec3, radius: &str, tangent_chain: bool) -> FeatureKind {
        FeatureKind::Fillet {
            edges: vec![crate::Position(at)],
            radius: radius.into(),
            body: None,
            style: crate::FilletStyle::Constant,
            fit: Default::default(),
            corner: Default::default(),
            setbacks: Vec::new(),
            turn: Default::default(),
            tangent_chain,
            edge_radii: Vec::new(),
        }
    }

    /// A 20 box's top edge along y rounded r2, then the top front edge (along x) rounded r3: that
    /// edge runs on tangentially into the round's end arc, so by default (tangent propagation)
    /// the arc is rounded with it, which an r3 round on an r2 arc cannot be: refused, saying that
    /// propagation blended the chain and how to blend the edge alone. With propagation off the
    /// edge is rounded alone, ending on the r2 round.
    #[test]
    fn a_propagated_chain_that_cannot_build_says_how_to_blend_the_edge_alone() {
        let build = |chain: bool| {
            let mut doc = Document::new("T");
            let solid = FeatureKind::Box {
                corner: crate::point_expr(Vec3::ZERO),
                length: "20".into(),
                width: "20".into(),
                height: "20".into(),
                operation: Operation::NewBody,
                centred: false,
            };
            doc.add_feature(solid, None).unwrap();
            doc.add_feature(fillet(Vec3::new(20.0, 10.0, 20.0), "2", true), None).unwrap();
            let last = doc.add_feature(fillet(Vec3::new(10.0, 0.0, 20.0), "3", chain), None).unwrap();
            let mut m = Model::new();
            m.evaluate(&doc);
            (m.result(last).and_then(|r| r.error.clone()), m.state().bodies.first().map(|b| b.body.face_count()))
        };
        let (on, _) = build(true);
        let on = on.expect("the propagated chain is refused");
        assert!(on.contains("with it off, tangent_chain: false, only the picked edges are blended"), "{on}");
        let (off, faces) = build(false);
        assert_eq!(off, None);
        assert_eq!(faces, Some(8));
    }

    /// A 40 × 16 × 10 plate ending in a half cylinder of radius 8: its sides run into the
    /// cylinder's wall tangentially, along smooth edges such as the one at x 40, y 0. A fillet of
    /// the back top edge and that smooth edge rounds the back edge and says it skipped the
    /// smooth one (#17919): 6400 + 320π − (1 − π/4)·16 mm³. Beside it a 10 mm cube, a body of its
    /// own: a fillet of a cube edge and the plate's smooth edge rounds the cube edge and leaves
    /// the plate as it was (the plate's picks, all smooth, are skipped, not refused). The smooth
    /// edge alone is refused.
    #[test]
    fn a_smooth_edge_among_the_picks_is_skipped_with_a_note() {
        use std::f64::consts::{FRAC_PI_4, PI};
        let build = |edges: &[Vec3]| {
            let mut doc = Document::new("T");
            let plate = FeatureKind::Box {
                corner: crate::point_expr(Vec3::ZERO),
                length: "40".into(),
                width: "16".into(),
                height: "10".into(),
                operation: Operation::NewBody,
                centred: false,
            };
            doc.add_feature(plate, None).unwrap();
            let end = FeatureKind::Cylinder {
                base: crate::point_expr(Vec3::new(40.0, 8.0, 0.0)),
                axis: crate::dir_expr(Vec3::Z),
                radius: "8".into(),
                height: "10".into(),
                operation: Operation::Join,
                references: Vec::new(),
            };
            doc.add_feature(end, None).unwrap();
            let cube = FeatureKind::Box {
                corner: crate::point_expr(Vec3::new(100.0, 0.0, 0.0)),
                length: "10".into(),
                width: "10".into(),
                height: "10".into(),
                operation: Operation::NewBody,
                centred: false,
            };
            doc.add_feature(cube, None).unwrap();
            let mut round = fillet(Vec3::ZERO, "1", true);
            if let FeatureKind::Fillet { edges: e, .. } = &mut round {
                *e = edges.iter().map(|p| crate::Position(*p)).collect();
            }
            let last = doc.add_feature(round, None).unwrap();
            let mut m = Model::new();
            m.evaluate(&doc);
            let r = m.result(last).cloned();
            let volumes: Vec<f64> = m.state().bodies.iter().map(|b| kernel::measure(&b.body).unwrap().volume).collect();
            (r, volumes)
        };
        let (plate, cube) = (6400.0 + 320.0 * PI, 1000.0);
        // A round of r 1 along an edge of length l takes (1 − π/4)·l mm³.
        let round = |l: f64| (1.0 - FRAC_PI_4) * l;
        for (picks, expected) in [
            ([Vec3::new(0.0, 8.0, 10.0), Vec3::new(40.0, 0.0, 5.0)], [plate - round(16.0), cube]),
            ([Vec3::new(105.0, 0.0, 10.0), Vec3::new(40.0, 0.0, 5.0)], [plate, cube - round(10.0)]),
        ] {
            let (r, volumes) = build(&picks);
            let r = r.expect("a result");
            assert_eq!(r.error, None, "{picks:?}");
            let warning = r.warning.unwrap_or_default();
            assert!(warning.contains("1 edge skipped: its faces meet tangentially") && warning.contains("(40, 0, 5)"), "{warning}");
            assert_eq!(volumes.len(), 2);
            for (v, e) in volumes.iter().zip(expected) {
                // Far tighter than the 2.1 mm³ the smaller round takes away.
                assert!((v - e).abs() < 1e-6 * e, "{picks:?}: {volumes:?} vs {expected:?}");
            }
        }
        let (alone, _) = build(&[Vec3::new(40.0, 0.0, 5.0)]);
        let error = alone.and_then(|r| r.error).unwrap_or_default();
        assert!(error.contains("is smooth"), "{error}");
    }

    /// A 40 × 40 × 10 plate with a ball socket: a sphere of radius 3 cut from its front face
    /// (y 0), centred on it at (20, 0, 5). The body stores the socket's rim as three arcs, a
    /// quarter, a half and a quarter. A round or a chamfer of the rim with tangent_chain: false,
    /// picked halfway along the first quarter, blends that quarter alone and says that the 2 arcs
    /// it runs on into are still sharp (#18827). A sphere joined on the plate's top face has a rim
    /// split by the sphere's seam, which SMLib rounds whole from one pick: no note.
    #[test]
    fn a_blend_without_tangent_propagation_says_which_smooth_continuations_it_left_sharp() {
        use std::f64::consts::FRAC_1_SQRT_2;
        let build = |sphere: (Vec3, Operation), blend: FeatureKind| {
            let mut doc = Document::new("T");
            let plate = FeatureKind::Box {
                corner: crate::point_expr(Vec3::ZERO),
                length: "40".into(),
                width: "40".into(),
                height: "10".into(),
                operation: Operation::NewBody,
                centred: false,
            };
            doc.add_feature(plate, None).unwrap();
            doc.add_feature(FeatureKind::Sphere { center: crate::point_expr(sphere.0), radius: "3".into(), operation: sphere.1 }, None).unwrap();
            let before = {
                let mut m = Model::new();
                m.evaluate(&doc);
                kernel::measure(&m.state().bodies[0].body).unwrap().volume
            };
            let last = doc.add_feature(blend, None).unwrap();
            let mut m = Model::new();
            m.evaluate(&doc);
            let r = m.result(last).cloned().expect("a result");
            assert_eq!(r.error, None);
            (r.warning, kernel::measure(&m.state().bodies[0].body).unwrap().volume - before)
        };
        let socket = (Vec3::new(20.0, 0.0, 5.0), Operation::Cut);
        let quarter = Vec3::new(20.0 + 3.0 * FRAC_1_SQRT_2, 0.0, 5.0 - 3.0 * FRAC_1_SQRT_2);
        let (warning, removed) = build(socket, fillet(quarter, "0.5", false));
        assert_eq!(
            warning.as_deref(),
            Some(
                "2 edges that the picked edge runs on into smoothly are still sharp because tangent_chain: false rounds only the \
                 picked edges (to blend them too, pick them in another fillet, which may fail where the edges meet)"
            )
        );
        // A quarter of the rim rounded: about a quarter of the 1.05 mm³ the whole rim's round takes.
        assert!(removed < -0.15 && removed > -0.4, "{removed}");
        let bevel = FeatureKind::Chamfer {
            edges: vec![crate::Position(quarter)],
            distance: "0.5".into(),
            distance2: None,
            angle: None,
            flip: false,
            body: None,
            tangent_chain: false,
        };
        let warning = build(socket, bevel).0.unwrap_or_default();
        assert!(warning.starts_with("2 edges that the picked edge runs on into smoothly are still sharp"), "{warning}");
        assert!(warning.contains("tangent_chain: false chamfers only the picked edges") && warning.contains("in another chamfer"), "{warning}");
        let ball = (Vec3::new(20.0, 20.0, 10.0), Operation::Join);
        let (warning, added) = build(ball, fillet(Vec3::new(20.0 + 3.0 * FRAC_1_SQRT_2, 20.0 + 3.0 * FRAC_1_SQRT_2, 10.0), "0.5", false));
        assert_eq!(warning, None);
        assert!(added > 0.0, "{added}");
    }

    /// A 40 × 40 × 10 plate with its four upright edges rounded r5: its top outline is one tangent
    /// chain of 8 edges, 4 lines and 4 arcs. A round of the front top line with tangent_chain:
    /// false says the other 7 are still sharp at every radius. From r1.6 up the round runs on past
    /// the line's ends and takes the middle of each neighbouring arc, but most of those arcs stay
    /// sharp, so they are counted too. A variable round, which runs along its picked edge whatever
    /// tangent_chain says, is told so rather than told to turn tangent_chain on.
    #[test]
    fn a_round_that_runs_past_its_edge_still_counts_the_edges_it_left_partly_sharp() {
        let build = |round: FeatureKind| {
            let mut doc = Document::new("T");
            let plate = FeatureKind::Box {
                corner: crate::point_expr(Vec3::new(-20.0, -20.0, 0.0)),
                length: "40".into(),
                width: "40".into(),
                height: "10".into(),
                operation: Operation::NewBody,
                centred: false,
            };
            doc.add_feature(plate, None).unwrap();
            let mut corners = fillet(Vec3::ZERO, "5", true);
            if let FeatureKind::Fillet { edges, .. } = &mut corners {
                *edges = [(20.0, 20.0), (-20.0, 20.0), (20.0, -20.0), (-20.0, -20.0)]
                    .into_iter()
                    .map(|(x, y)| crate::Position(Vec3::new(x, y, 5.0)))
                    .collect();
            }
            doc.add_feature(corners, None).unwrap();
            let last = doc.add_feature(round, None).unwrap();
            let mut m = Model::new();
            m.evaluate(&doc);
            let r = m.result(last).cloned().expect("a result");
            assert_eq!(r.error, None);
            r.warning.unwrap_or_default()
        };
        let front = Vec3::new(0.0, -20.0, 10.0);
        for radius in ["1.4", "1.6", "2.5", "3.5"] {
            let warning = build(fillet(front, radius, false));
            assert!(warning.starts_with("7 edges that the picked edge runs on into smoothly are still sharp"), "r{radius}: {warning}");
        }
        let mut variable = fillet(front, "1", false);
        if let FeatureKind::Fillet { style, .. } = &mut variable {
            *style = crate::FilletStyle::Variable {
                radius2: "0.5".into(),
                start: crate::Position(Vec3::new(-15.0, -20.0, 10.0)),
                mid: Vec::new(),
                smooth: false,
            };
        }
        let warning = build(variable);
        assert!(warning.contains("still sharp because a variable or spline round runs along its picked edge only"), "{warning}");
        assert!(!warning.contains("tangent_chain"), "{warning}");
    }
}

#[cfg(test)]
#[path = "eval_cost_tests.rs"]
pub(crate) mod cost_tests;

#[cfg(test)]
mod body_name_tests {
    use super::*;

    /// Imported bodies get the names one-at-a-time naming gives them, in time linear in the
    /// bodies: a board file with thousands of same-named parts once took minutes (#883).
    #[test]
    fn many_same_named_bodies_are_named_in_linear_time() {
        let mut st = ModelState::default();
        let body = kernel::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        for name in ["R", "R (3)", "C"] {
            st.bodies.push(ModelBody::new(name.into(), body.clone(), 1));
        }
        // The same names as asking the bodies one at a time.
        let mut names = BodyNames::new(&st);
        let mut slow = st.clone();
        for base in ["R", "R", "C", " ", "Body", "R", "R (2)", "X"] {
            let expected = unique_body_name(&slow, base);
            assert_eq!(names.unique(base), expected);
            slow.bodies.push(ModelBody::new(expected, body.clone(), 2));
        }
        // 50,000 parts named alike: milliseconds (one-at-a-time naming would take hours).
        let began = std::time::Instant::now();
        let mut names = BodyNames::new(&ModelState::default());
        let last = (0..50_000).map(|_| names.unique("R_0603")).last().unwrap();
        assert_eq!(last, "R_0603 (50000)");
        assert!(began.elapsed() < std::time::Duration::from_secs(5), "{:?}", began.elapsed());
    }

    /// Copies of a renamed pattern numbered after its highest stored name (#372) take the first
    /// free number from there, as searching the bodies one name at a time gave them.
    #[test]
    fn pattern_copies_past_the_stored_names_take_the_first_free_numbers() {
        let mut st = ModelState::default();
        let body = kernel::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        for name in ["Pin", "Pin (5)", "Pin (7)"] {
            st.bodies.push(ModelBody::new(name.into(), body.clone(), 1));
        }
        let mut names = BodyNames::new(&st);
        let got: Vec<String> = (0..4).map(|_| names.first_free("Pin", 5)).collect();
        assert_eq!(got, ["Pin (6)", "Pin (8)", "Pin (9)", "Pin (10)"]);
        // After first-free naming from 2, a later start never hands out a name twice.
        assert_eq!(names.unique("Pin"), "Pin (2)");
        assert_eq!(names.first_free("Pin", 2), "Pin (3)");
    }

    /// A 10 mm cube from `x` along x.
    fn cube_at(x: f64) -> Body {
        kernel::box_solid(Vec3::new(x, 0.0, 0.0), Vec3::new(x + 10.0, 10.0, 10.0)).unwrap()
    }

    /// Base, made by feature 3, moved into component 5 by a transfer that turned it, under the
    /// definition name Def.
    fn transferred_base() -> ModelBody {
        let mut base = ModelBody::new("Base".into(), cube_at(0.0), 3);
        base.component = 5;
        base.transferred = true;
        base.source_name = Some("Def".into());
        base.source_transform = crate::rigid(Vec3::new(1.0, 2.0, 3.0), Vec3::ZERO, Vec3::Z, 0.5);
        base
    }

    /// A body a feature changes keeps its name, feature, definition and frame: a later step of
    /// the same feature still meets it in its component, where it is placed (#19451).
    #[test]
    fn a_changed_body_keeps_its_definition_and_frame() {
        let base = transferred_base();
        let b = base.remade(crate::lineage::Traced { body: cube_at(1.0), lineage: None, recipe: None });
        assert_eq!((b.name.as_str(), b.feature, b.source_name.as_deref()), ("Base", 3, Some("Def")));
        assert_eq!((b.component, b.transferred, b.source_transform), (5, true, base.source_transform));
    }
}

/// The parts of a body split by a plane, on its positive side first (only the non-empty ones).
/// Each part's cut face is named `section` after the half-space box that cut it, so it takes an
/// attachment that follows the plane (#565); the box's other faces never reach the part.
fn split_by_plane_traced(input: &Traced, plane: &Plane) -> Result<Vec<Traced>> {
    let plane = &square_to_axes(plane);
    let mut parts = Vec::new();
    for positive in [true, false] {
        let (part, tool) = kernel::half_space(&input.body, plane, positive)?;
        let Some(part) = part else { continue };
        // The box's cap in the plane: its start cap on the positive side, its end cap on the other.
        let section = Some("section".to_string());
        let caps = if positive { [section, None] } else { [None, section] };
        let tool = crate::lineage::ProfileRoles { sketch: None, region: None, caps, cap_synonyms: [None, None], thin: None }.tool(tool, None);
        parts.push(Traced::step(part, &[input, &tool]));
    }
    Ok(parts)
}

/// `plane` with its directions' parts below rounding (1e-12) along a world axis dropped. A flat
/// face picked in the viewport gives its plane tilted by rounding, about 2e-15: split by that,
/// the face lying in the plane no longer meets the cut exactly, and the boolean nudges its tool
/// and leaves a slab a few µm thick under the face (#6762). A plane with nothing to drop stays
/// as it is.
fn square_to_axes(plane: &Plane) -> Plane {
    match solidcore_ops::affine::axis_squared_axes(plane.x.into(), plane.y.into(), 1e-12) {
        Some([x, y]) => Plane { origin: plane.origin, x: Vec3::new(x[0], x[1], x[2]), y: Vec3::new(y[0], y[1], y[2]) },
        None => *plane,
    }
}

/// The pieces of a split, a side in pieces that do not touch made one body each (#6762): a cap's
/// skirt and tube above its plate are two pieces, not one. Only a side in more pieces than the
/// body had is taken apart, so the pieces of a join stay together as a cut keeps them. The
/// first piece of each side keeps that side's place and the further pieces come after them all,
/// so the names a split gave its sides before stay on the same pieces. A side whose pieces
/// cannot be worked out stays one body.
fn split_side_lumps(whole: &Body, parts: Vec<Traced>) -> Vec<Traced> {
    let had = whole.lumps().map_or(1, |l| l.len());
    let (mut sides, mut further) = (Vec::with_capacity(parts.len()), Vec::new());
    for part in parts {
        let pieces = part.body.lumps().unwrap_or_default();
        if pieces.len() <= had {
            sides.push(part);
            continue;
        }
        let lineages = crate::lineage::Lineage::lumps(&part.body, part.lineage.as_ref(), &pieces);
        let recipe = part.recipe;
        let mut it = pieces.into_iter().zip(lineages).map(|(body, lineage)| Traced { body, lineage, recipe });
        sides.extend(it.next());
        further.extend(it);
    }
    sides.extend(further);
    sides
}

/// The plane a straight sketch line sweeps along its sketch's normal: through the line, its x
/// along the line and its y along the normal (a Split Body tool, #1552).
pub fn sketch_line_plane(st: &ModelState, sketch: u64, line: &str) -> Result<Plane> {
    let ss = st.sketch(sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
    let ci = ss.sketch.curve_index(line).ok_or_else(|| DocError::Unknown(format!("curve `{line}`")))?;
    let (a, b) = match ss.sketch.segs(ci).as_slice() {
        [solidcore_record::Seg2::Line { a, b }] => (*a, *b),
        _ => return Err(DocError::Invalid(format!("`{line}` is not a straight line: split by a sketch line"))),
    };
    let (wa, wb) = (ss.plane.to_world(a), ss.plane.to_world(b));
    let x = (wb - wa).normalized().ok_or_else(|| DocError::Invalid(format!("`{line}` has no length")))?;
    Ok(Plane { origin: wa, x, y: ss.plane.normal() })
}

#[cfg(test)]
mod named_face_lookup_tests {
    use super::*;
    use crate::naming::FaceHit;

    /// A row of `n` 10 mm cubes `Part0`, `Part1`… 20 mm apart along x, without history: their
    /// faces are named from geometry, which meshes them.
    fn parts(n: usize) -> ModelState {
        let mut st = ModelState::default();
        for i in 0..n {
            let x = 20.0 * i as f64;
            let cube = kernel::box_solid(Vec3::new(x, 0.0, 0.0), Vec3::new(x + 10.0, 10.0, 10.0)).unwrap();
            st.bodies.push(ModelBody::new(format!("Part{i}"), cube, 1));
        }
        st
    }

    /// The name of the face of `body` at `p`.
    fn name_at(st: &ModelState, body: &str, p: Vec3) -> String {
        let b = st.body(body).unwrap();
        let face = crate::appearance::face_index_at(b, p).unwrap();
        crate::naming::face_names(b)[face].clone()
    }

    /// Everything a lookup found, to compare two lookups.
    fn found(hit: Option<FaceHit>) -> String {
        format!("{hit:?}")
    }

    /// A shell late in a long timeline spent seconds meshing every body of the model to find its
    /// face by name (#287). A feature that names its body finds the face there without naming
    /// the other bodies, and finds what looking over every body finds.
    #[test]
    fn a_face_is_found_on_the_body_the_feature_names_without_meshing_the_others() {
        let st = parts(12);
        let top = Vec3::new(145.0, 5.0, 10.0);
        let name = name_at(&st, "Part7", top);
        let hit = crate::name_lookup::locate_face_on_body(&st, Some("Part7"), &name, top);
        for b in st.bodies.iter().filter(|b| b.name != "Part7") {
            assert!(b.mesh.get().is_none(), "{} was meshed", b.name);
        }
        let h = hit.unwrap();
        assert_eq!(h.body, 7);
        assert_eq!(found(hit), found(crate::naming::locate_face(&st, &name, top)));
        // Looking over every body names (and so meshes) them all.
        assert!(st.bodies.iter().all(|b| b.mesh.get().is_some()));
    }

    /// A name the named body has no face of (it moved to another body: a combine took the face),
    /// a body that is not there, no body, a name saved before format 14 or a name no face has:
    /// looked for over every body, as without a body.
    #[test]
    fn a_name_not_on_the_named_body_is_looked_for_over_every_body() {
        let st = parts(4);
        let top = Vec3::new(45.0, 5.0, 10.0);
        let name = name_at(&st, "Part2", top);
        for name in [name.clone(), format!("{}{name}", crate::naming::LEGACY_MARK), "Part2:face:99".into()] {
            let everywhere = found(crate::naming::locate_face(&st, &name, top));
            for body in [Some("Part1"), Some("Part2"), Some("Nope"), None] {
                assert_eq!(found(crate::name_lookup::locate_face_on_body(&st, body, &name, top)), everywhere, "{name} on {body:?}");
            }
        }
        let moved = crate::name_lookup::locate_face_on_body(&st, Some("Part1"), &name, top).unwrap();
        assert_eq!(moved.body, 2);
    }

    /// A copy of a body has its source's face names. Over every body the name finds the face on
    /// both, and takes the one the point lies on; a point on neither finds no face (it is never
    /// told apart by the nearest). A feature on the copy takes the copy's face.
    #[test]
    fn the_same_face_on_a_copy_is_not_a_piece_of_the_named_bodys_face() {
        let mut st = parts(2);
        let source = st.bodies[0].clone();
        st.bodies[1].names = crate::naming::NameCell::same(&source);
        let top = Vec3::new(25.0, 5.0, 10.0);
        let name = name_at(&st, "Part1", top);
        assert_eq!(name, name_at(&st, "Part0", Vec3::new(5.0, 5.0, 10.0)));
        let on_copy = crate::name_lookup::locate_face_on_body(&st, Some("Part1"), &name, top).unwrap();
        let everywhere = crate::naming::locate_face(&st, &name, top).unwrap();
        assert_eq!((on_copy.body, on_copy.face, on_copy.point), (everywhere.body, everywhere.face, everywhere.point));
        assert_eq!(on_copy.note(&name), None);
        assert!(crate::naming::locate_face(&st, &name, Vec3::new(15.0, 5.0, 30.0)).is_none());
        assert_eq!(crate::name_lookup::locate_face_on_body(&st, Some("Part1"), &name, Vec3::new(15.0, 5.0, 30.0)).map(|hit| hit.body), Some(1));
    }

    #[test]
    fn planar_normal_at_evaluates_exact_brep_plane() {
        let b = solidcraft_kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let n = more::planar_normal_at(&b, Vec3::new(5.0, 5.0, 10.0));
        assert_eq!(n, Some(Vec3::Z));
    }

    #[test]
    fn body_at_picks_nearest_body_using_exact_bounds() {
        let st = parts(3);
        let hit = body_at(&st, &None, &[Vec3::new(22.0, 5.0, 5.0)]).unwrap();
        assert_eq!(st.bodies[hit].name, "Part1");
    }
}

#[cfg(test)]
mod kept_refused_tests {
    use super::*;

    /// #17689: a refused result names the inputs that already failed SMLib's check: its own
    /// former self and the bodies the feature used up, never another body the same feature
    /// changed beside it.
    #[test]
    fn a_refusal_names_only_the_results_own_inputs_that_failed_the_check() {
        let text = include_str!("../../../tests/step-regressions/block_edge_off_its_face.step");
        let bad = kernel::step_import(text).unwrap().bodies.remove(0).body;
        assert!(kernel::failing_check(&bad).unwrap().is_some(), "the block fails the check");
        let good = kernel::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        assert!(kernel::failing_check(&good).unwrap().is_none(), "the box passes it");
        let e = kernel::KernelError::Failed("refused".into());
        let msg = |name: &str, inputs: &[(String, Body, bool)]| kept_refused(name, &e, inputs).to_string();
        // Both moved by one feature: the box's result is not blamed on the block.
        let both = [("Bloco".to_string(), bad.clone(), false), ("Box1".to_string(), good.clone(), false)];
        assert!(!msg("Box1", &both).contains("before this feature"), "{}", msg("Box1", &both));
        assert!(msg("Bloco", &both).contains("body `Bloco` (SMLib's check fails it as it was before this feature)"), "{}", msg("Bloco", &both));
        // The block used up by a join into the box: named.
        let joined = [("Bloco".to_string(), bad, true), ("Box1".to_string(), good, false)];
        assert!(
            msg("Box1", &joined).contains("body `Box1` (SMLib's check fails body `Bloco` as it was before this feature)"),
            "{}",
            msg("Box1", &joined)
        );
    }

    /// A panic caught while checking a kept body stays an internal error with the body named, so
    /// the evaluation does not cache it as an ordinary refusal.
    #[test]
    fn a_panic_in_the_check_stays_internal() {
        let good = kernel::box_solid(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        let inputs = [("Box1".to_string(), good, false)];
        let internal = kept_refused("Box1", &kernel::KernelError::Internal("SMLib copy: boom".into()), &inputs);
        assert!(matches!(&internal, kernel::KernelError::Internal(m) if m == "body `Box1`: SMLib copy: boom"), "{internal:?}");
        let refused = kept_refused("Box1", &kernel::KernelError::Failed("refused".into()), &inputs);
        assert!(matches!(refused, kernel::KernelError::Failed(_)), "{refused:?}");
    }
}

#[cfg(test)]
mod path_station_tests {
    use super::*;

    /// A point repeated in a path (a flattened spline samples one twice) leaves a segment with no
    /// length and no direction. The frame along the path is the one it has without that
    /// segment: it does not turn through a made-up direction at the repeat, which would roll
    /// every copy after it. Repeats at the path's ends keep the stations clamped to its ends.
    #[test]
    fn a_repeated_point_does_not_turn_the_path_frame() {
        let line = |a: Vec3, b: Vec3| kernel::PathSeg::Line { a, b };
        let (a, b, c) = (Vec3::ZERO, Vec3::new(0.0, 10.0, 0.0), Vec3::new(5.0, 10.0, 5.0));
        let clean = [line(a, b), line(b, c)];
        let repeated = [line(a, a), line(a, b), line(b, b), line(b, c), line(c, c)];
        let total = 10.0 + 50f64.sqrt();
        for d in [0.0, 5.0, 10.0, 12.0, total, total + 1.0] {
            let (p, r) = path_station(&clean, d, Vec3::Z);
            let (q, s) = path_station(&repeated, d, Vec3::Z);
            assert!(p.dist(q) < 1e-12, "at {d}: {p:?} vs {q:?}");
            for (row, other) in r.iter().zip(&s) {
                for (x, y) in row.iter().zip(other) {
                    assert!((x - y).abs() < 1e-12, "at {d}: {r:?} vs {s:?}");
                }
            }
        }
    }
}
