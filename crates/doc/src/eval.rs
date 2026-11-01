//! Timeline evaluation.

#[path = "placed_mesh.rs"]
mod placed_mesh;
#[path = "placement_reuse.rs"]
mod placement_reuse;

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};
use solidcraft_geom::tess::TessPlan;
use solidcraft_geom::{Mesh, Plane, Region2, UnitVec3, Vec2, Vec3, regions_apart};
use solidcraft_kernel::{self as kernel, Body, BoolOp};
use solidcraft_sketch::{CurveKind, Profile, SolveReport, find_profiles};

use crate::expr::{self, Kind, Value};
use crate::lineage::Traced;
use crate::{AxisRef, DocError, Document, Feature, FeatureId, FeatureKind, HoleKind, Operation, PlaneRef, ProfileSel, Result};

type JointFaces = (Body, kernel::Result<Arc<[kernel::FaceInfo]>>);

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
    // Timeline snapshots are cloned before meshes are requested. Share the cell itself so
    // meshing any snapshot also warms every unchanged copy of this body.
    mesh: Arc<OnceLock<Arc<Mesh>>>,
    /// Joint face measurements use their own tolerance, independent of display quality.
    /// Shared by snapshots of this geometry; the body identity guards direct replacement.
    joint_faces: Arc<OnceLock<JointFaces>>,
    /// Lazy canonical reference triangles for a proper rigid occurrence placement.
    mesh_placement: Option<Arc<placed_mesh::PlacedMesh>>,
    /// The display plan (none when the body has no plan: a mesh body, or one the planner
    /// refuses), shared like the mesh. A GPU viewport draws it.
    plan: Arc<[OnceLock<Option<Arc<TessPlan>>>; 4]>,
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
    /// The component the body belongs to (set after each feature).
    pub component: u64,
    /// A timeline transfer has converted this body into its evaluated owner's frame.
    pub transferred: bool,
    /// The look it was imported with, carried through later features.
    pub imported: Option<Arc<crate::appearance::ImportedLook>>,
    /// [`Body::retained_bytes`] of `body`, measured when a feature made or changed it (0: not
    /// measured), so the evaluation cache's budget does not walk every body after every feature.
    bytes: usize,
    /// In a world state: the [`ModelBody::geometry_key`] of the model body this one places (0: none).
    source_key: u64,
    /// [`Body::outline_bounds`] of `body` (with the body it was measured on), worked out once
    /// and shared by clones.
    outline: Arc<OnceLock<(Body, solidcraft_geom::Aabb3)>>,
    /// [`kernel::measure`] of `body` (with the body it was measured on), worked out once and
    /// shared by clones: inspecting a design again measures only the bodies that changed.
    measured: Arc<OnceLock<(Body, kernel::Result<kernel::BodyMeasure>)>>,
    /// [`Body::is_surface`] of `body` (with the body it was measured on), worked out once and
    /// shared by clones.
    surface: Arc<OnceLock<(Body, bool)>>,
}

impl ModelBody {
    pub fn new(name: String, body: Body, feature: u64) -> Self {
        ModelBody {
            name,
            source_name: None,
            source_transform: crate::IDENTITY,
            body,
            feature,
            mesh: Arc::new(OnceLock::new()),
            joint_faces: Arc::new(OnceLock::new()),
            mesh_placement: None,
            plan: Arc::new(Default::default()),
            plan_mesh: Arc::new(Default::default()),
            key: 0,
            names: Default::default(),
            made: Default::default(),
            component: 0,
            transferred: false,
            imported: None,
            bytes: 0,
            source_key: 0,
            outline: Arc::new(OnceLock::new()),
            measured: Arc::new(OnceLock::new()),
            surface: Arc::new(OnceLock::new()),
        }
    }

    /// A surface body: some shell does not close ([`Body::is_surface`]), worked out once.
    pub fn is_surface(&self) -> bool {
        match self.surface.get_or_init(|| (self.body.clone(), self.body.is_surface())) {
            (measured, s) if measured.same(&self.body) => *s,
            // The body was replaced in this cell's holder since: look again.
            _ => self.body.is_surface(),
        }
    }

    /// Face measurements for joint origins, at their original chord tolerance. Pose changes
    /// and scratch copies share them; a geometry edit makes a new body and cache.
    pub(crate) fn joint_faces(&self) -> Result<Arc<[kernel::FaceInfo]>> {
        let measure = || {
            #[cfg(test)]
            cost_tests::count(|c| c.joint_faces += 1);
            let tol = (self.body.size() * 2e-3).max(1e-3);
            self.body.faces(tol).map(Arc::from)
        };
        let (body, faces) = self.joint_faces.get_or_init(|| (self.body.clone(), measure()));
        if body.same(&self.body) { faces.clone().map_err(Into::into) } else { measure().map_err(Into::into) }
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
                None => self.display_plan_at(q).and_then(|plan| self.body.display_mesh_of(&plan).ok()).map(Arc::new),
            })
            .clone()
    }

    /// Identity of this body's display geometry, without meshing it: the cell its mesh is kept
    /// in, shared by every clone of the body (timeline snapshots, world states that place it
    /// unchanged) and, while it is held, by no other body.
    pub fn display_identity(&self) -> Arc<dyn std::any::Any + Send + Sync> {
        self.mesh.clone()
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
    /// points, moved.
    pub fn mesh_bounds(&self) -> solidcraft_geom::Aabb3 {
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
            let started = std::thread::Builder::new().name("plan-mesh".into()).spawn(move || {
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

    /// The display plan and its mesh `k` steps finer than `q` (view-dependent refinement, #1320;
    /// see [`crate::display_quality::refinement_for`]). Step 0 is the cached plan at `q`; finer
    /// steps are made on each call and kept by the viewport, which evicts them when it runs over
    /// its budget. `None` when the body has no plan at that step.
    pub fn refined_display(&self, q: crate::display_quality::DisplayQuality, k: u8) -> Option<(Arc<TessPlan>, Arc<Mesh>)> {
        if k == 0 {
            return Some((self.display_plan_at(q)?, self.plan_mesh_at(q)?));
        }
        let plan = match self.body.display_plan_angular(q.refined_chord(self.body.size(), k), q.refined_angle(k)) {
            Ok(plan) => plan,
            Err(e) => {
                log::debug!("{}: no display plan refined {k} steps ({e})", self.name);
                return None;
            }
        };
        let mesh = self.body.display_mesh_of(&plan).ok()?;
        Some((Arc::new(plan), Arc::new(mesh)))
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
    /// Open and untied ends of its profile curves (where a profile can fail).
    pub ends: solidcraft_sketch::EndReport,
}

/// A cosmetic thread: on which cylinder, its size, and the threaded part of the axis.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ThreadInfo {
    pub feature: u64,
    pub body: String,
    pub designation: String,
    pub major_diameter: f64,
    /// Radius of the actual cylindrical face when the cosmetic thread is evaluated.
    pub radius: f64,
    pub pitch: f64,
    pub internal: bool,
    #[serde(default)]
    pub handedness: crate::ThreadHandedness,
    pub axis_point: Vec3,
    pub axis: UnitVec3,
    /// Threaded part along the axis from `axis_point`.
    pub start: f64,
    pub end: f64,
}

/// ISO metric coarse threads: (nominal diameter, pitch).
const ISO_COARSE: [(f64, f64); 22] = [
    (1.6, 0.35),
    (2.0, 0.4),
    (2.5, 0.45),
    (3.0, 0.5),
    (4.0, 0.7),
    (5.0, 0.8),
    (6.0, 1.0),
    (8.0, 1.25),
    (10.0, 1.5),
    (12.0, 1.75),
    (14.0, 2.0),
    (16.0, 2.0),
    (18.0, 2.5),
    (20.0, 2.5),
    (22.0, 2.5),
    (24.0, 3.0),
    (27.0, 3.0),
    (30.0, 3.5),
    (36.0, 4.0),
    (42.0, 4.5),
    (48.0, 5.0),
    (56.0, 5.5),
];

/// Nominal diameters and coarse pitches (mm) used for automatic ISO metric thread sizing.
/// Other diameter/pitch pairs can be supplied as explicit designations.
pub fn metric_thread_sizes() -> &'static [(f64, f64)] {
    &ISO_COARSE
}

/// "M8" or "M8x1" → (nominal diameter, pitch).
pub fn parse_metric_thread(s: &str) -> Option<(f64, f64)> {
    let t = s.trim().to_ascii_uppercase();
    let body = t.strip_prefix('M')?;
    let (d, p) = match body.split_once(['X', '×']) {
        Some((d, p)) => (d.trim().parse::<f64>().ok()?, Some(p.trim().parse::<f64>().ok()?)),
        None => (body.trim().parse::<f64>().ok()?, None),
    };
    let pitch = match p {
        Some(p) => p,
        None => ISO_COARSE.iter().find(|(n, _)| (n - d).abs() < 1e-9)?.1,
    };
    (d.is_finite() && pitch.is_finite() && d > 0.0 && pitch > 0.0 && pitch < d).then_some((d, pitch))
}

/// The metric thread a cylinder of this diameter takes: a shaft is the major diameter, a hole
/// the tap drill (major − pitch).
fn thread_for(diameter: f64, internal: bool) -> Option<(f64, f64)> {
    ISO_COARSE
        .iter()
        .map(|&(n, p)| (n, p, if internal { n - p } else { n }))
        .min_by(|a, b| (a.2 - diameter).abs().total_cmp(&(b.2 - diameter).abs()))
        .filter(|(_, _, want)| (want - diameter).abs() <= 0.15 * diameter)
        .map(|(n, p, _)| (n, p))
}

pub(crate) fn format_thread(d: f64, p: f64) -> String {
    let coarse = ISO_COARSE.iter().any(|(n, q)| (n - d).abs() < 1e-9 && (q - p).abs() < 1e-9);
    let num = |x: f64| if (x - x.round()).abs() < 1e-9 { format!("{}", x.round()) } else { format!("{x}") };
    if coarse { format!("M{}", num(d)) } else { format!("M{}x{}", num(d), num(p)) }
}

/// The automatic ISO metric designation for a cylindrical face, using the same fit check as
/// a thread feature without an explicit size. This query does not change the model.
pub fn suggested_thread(st: &ModelState, p: Vec3) -> Result<String> {
    thread_on(st, 0, p, None, None, false, crate::ThreadHandedness::Right).map(|t| t.designation)
}

/// A cosmetic thread on the cylindrical face at `p`. A `turned` (modelled) thread also fits a
/// shaft thicker than it, which it turns down to its major diameter first.
fn thread_on(
    st: &ModelState,
    feature: u64,
    p: Vec3,
    designation: Option<&str>,
    length: Option<f64>,
    turned: bool,
    handedness: crate::ThreadHandedness,
) -> Result<ThreadInfo> {
    thread_on_impl(st, feature, p, designation, length, false, turned, handedness)
}

// Tapped holes admit verified Unified and parallel pipe sizes; the separate Thread feature remains metric,
// including when an untrusted design requests modelled geometry.
fn thread_on_impl(
    st: &ModelState,
    feature: u64,
    p: Vec3,
    designation: Option<&str>,
    length: Option<f64>,
    tapped: bool,
    turned: bool,
    handedness: crate::ThreadHandedness,
) -> Result<ThreadInfo> {
    let (body, cyl) = st
        .bodies
        .iter()
        .filter_map(|b| kernel::cylinder_face_at(&b.body, p).map(|c| (b.name.clone(), c)))
        .min_by(|a, b| {
            let dist = |c: &kernel::CylinderFace| ((p - c.axis_point - c.axis * (p - c.axis_point).dot(c.axis)).len() - c.radius).abs();
            dist(&a.1).total_cmp(&dist(&b.1))
        })
        .ok_or_else(|| DocError::Invalid("no cylindrical face there to thread".into()))?;
    let dia = cyl.radius * 2.0;
    let tapped_size = if tapped {
        let name = designation.ok_or_else(|| DocError::Invalid("a tapped hole needs a designation".into()))?;
        Some(
            crate::parse_tapped_thread(name)
                .ok_or_else(|| DocError::Invalid(format!("unknown tapped thread `{name}` (ISO metric or a catalogued UNC/UNF, G or Rp size)")))?,
        )
    } else {
        None
    };
    let (major, pitch) = match &tapped_size {
        Some(size) => (size.major_diameter, size.pitch),
        None => match designation {
            Some(s) => parse_metric_thread(s).ok_or_else(|| DocError::Invalid(format!("unknown thread `{s}` (ISO metric, like M8 or M8x1)")))?,
            None => thread_for(dia, cyl.internal).ok_or_else(|| DocError::Invalid(format!("no metric thread fits a {dia:.2} mm cylinder")))?,
        },
    };
    let name = tapped_size.as_ref().map(|s| s.designation.clone()).unwrap_or_else(|| format_thread(major, pitch));
    let fits = if let Some(size) = tapped_size.as_ref().filter(|s| s.family != crate::TappedThreadFamily::Metric) {
        // Fit the recommended physical drill, without borrowing an ISO minor-diameter rule.
        cyl.internal && (size.tap_drill - dia).abs() < 0.15 * major
    } else if cyl.internal {
        (major - pitch * 1.0825 - dia).abs() < 0.2 * major || (major - pitch - dia).abs() < 0.15 * major
    } else {
        (major - dia).abs() < 0.15 * major || (turned && dia > major)
    };
    if !fits {
        return Err(DocError::Invalid(format!("an {} thread does not fit a {dia:.2} mm {}", name, if cyl.internal { "hole" } else { "shaft" })));
    }
    // From the end of the face nearest the point.
    let s = (p - cyl.axis_point).dot(cyl.axis);
    let (mut a, mut b) = (cyl.start, cyl.end);
    if let Some(l) = length {
        if !(l > 0.0) {
            return Err(DocError::Invalid("thread length must be positive".into()));
        }
        if (s - a).abs() <= (b - s).abs() {
            b = (a + l).min(b);
        } else {
            a = (b - l).max(a);
        }
    }
    Ok(ThreadInfo {
        feature,
        body,
        designation: name,
        major_diameter: major,
        radius: cyl.radius,
        pitch,
        internal: cyl.internal,
        handedness,
        axis_point: cyl.axis_point,
        axis: cyl.axis,
        start: a,
        end: b,
    })
}

/// The helical space between the teeth of a modelled ISO metric thread (ISO 68-1 basic
/// profile, 60° flanks): cut from the shaft (external) or the hole wall (internal), running from
/// `start` to `end` along the axis. `radius` is the face's own radius; `open` says which ends
/// of the threaded length are free ends of the face.
fn thread_groove(t: &ThreadInfo, radius: f64, open: (bool, bool)) -> Result<solidcraft_geom::Mesh> {
    let (p, d) = (t.pitch, t.major_diameter);
    let h = p * 3f64.sqrt() / 2.0;
    let slope = (std::f64::consts::PI / 6.0).tan();
    let len = t.end - t.start;
    if !(p > 1e-3 && len > 1e-6) {
        return Err(DocError::Invalid("thread too short".into()));
    }
    if len / p > 190.0 {
        return Err(DocError::Invalid(format!("a modelled thread is limited to 190 turns ({:.0} mm at this pitch); use a cosmetic one", 190.0 * p)));
    }
    // How deep the groove cuts into the material: from the hole wall out to the major diameter,
    // or from the shaft's surface in to the root. Next to nothing (a hole as wide as the thread,
    // #995) is refused: the faceted cut would leave the face faceted and unthreaded.
    let basic = 5.0 * h / 8.0;
    let depth = if t.internal { d / 2.0 - radius } else { radius - (d / 2.0 - basic) };
    if !(depth >= basic / 4.0) {
        let (m, dia) = (&t.designation, 2.0 * radius);
        let cut = format!("it would cut {:.2} mm of the thread's {basic:.2} mm depth", depth.max(0.0));
        let alt = thread_for(dia, t.internal).map(|(n, q)| format_thread(n, q)).filter(|a| a != m);
        let shown = show_mm(dia);
        return Err(DocError::Invalid(if t.internal {
            let alt = alt.map(|a| format!(", or thread this hole {a}")).unwrap_or_default();
            format!("this {shown} mm hole is too wide for a modelled {m} thread: {cut}; drill {} mm for {m}{alt}", show_mm(d - p))
        } else {
            let alt = alt.map(|a| format!(", or thread this shaft {a}")).unwrap_or_default();
            format!("this {shown} mm shaft is too thin for a modelled {m} thread: {cut}; make the shaft {} mm for {m}{alt}", show_mm(d))
        }));
    }
    // (radius, half width of the groove) at its two ends: a trapezoid in the axial plane.
    let (r0, w0, r1, w1) = if t.internal {
        // From inside the hole wall out to the major diameter, where the bolt's crest sits
        // (the root flat is P/8 wide).
        let r_in = radius.min(d / 2.0 - 5.0 * h / 8.0) - 0.1 * p;
        (r_in, (p / 8.0 + 2.0 * (d / 2.0 - r_in) * slope) / 2.0, d / 2.0, p / 16.0)
    } else {
        // From the root (5H/8 under the major diameter, flat P/4 wide) out past the surface.
        let r_root = d / 2.0 - 5.0 * h / 8.0;
        let r_out = radius.max(d / 2.0) + 0.05 * p;
        (r_root, p / 8.0, r_out, (p / 4.0 + 2.0 * (r_out - r_root) * slope) / 2.0)
    };
    if w0.max(w1) * 2.0 >= p * 0.98 {
        // The groove would be wider than a pitch: the face is far inside the thread (a hole
        // narrower than its minor diameter, a shaft thicker than its major one, #2688).
        let (m, dia) = (&t.designation, 2.0 * radius);
        let alt = thread_for(dia, t.internal).map(|(n, q)| format_thread(n, q)).filter(|a| a != m);
        let shown = show_mm(dia);
        return Err(DocError::Invalid(if t.internal {
            let alt = alt.map(|a| format!(", or thread this hole {a}")).unwrap_or_default();
            format!(
                "this {shown} mm hole is too narrow for a modelled {m} thread (its teeth would fill the hole); drill {} mm for {m}{alt}",
                show_mm(d - p)
            )
        } else {
            let alt = alt.map(|a| format!(", or thread this shaft {a}")).unwrap_or_default();
            format!("this {shown} mm shaft is too thick for a modelled {m} thread; make the shaft {} mm for {m}{alt}", show_mm(d))
        }));
    }
    let k = t.axis;
    let u = k.cross(if k.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) });
    let u = u.normalized().ok_or_else(|| DocError::Invalid("thread axis".into()))?;
    // Past a free end of the face the groove runs out into the air; elsewhere it stops square
    // at the threaded length's end.
    let (s0, s1) = (if open.0 { t.start - p } else { t.start }, if open.1 { t.end + p } else { t.end });
    let base = t.axis_point + k * s0;
    let plane = Plane { origin: base, x: u, y: *k };
    let section = [Vec2::new(r0, -w0), Vec2::new(r1, -w1), Vec2::new(r1, w1), Vec2::new(r0, w0)];
    // Keep radial chord error below 1% of the pitch, rather than a fixed fraction of the
    // diameter. At 48 stations a large fine thread cuts too deeply; its outer tool facets
    // can even dip under the shaft surface. The existing coarse sizes still use 48.
    let outer = r0.abs().max(r1.abs());
    if !outer.is_finite() {
        return Err(DocError::Invalid("thread dimensions are too large".into()));
    }
    let angle = (1.0 - (p * 0.01 / outer).min(1.0)).acos();
    let stations = (std::f64::consts::PI / angle).ceil().max(48.0);
    if !stations.is_finite() || stations > 720.0 {
        return Err(DocError::Invalid("the modelled thread is too fine for this diameter; use a cosmetic thread".into()));
    }
    // The kernel rotates positively about its sweep axis. Reversing both that axis and the
    // pitch mirrors the angular motion while the lift stays +k * p per turn. The physical
    // axis, start/end, radial profile and station count remain exactly as for a right thread.
    let (sweep_axis, sweep_pitch) = match t.handedness {
        crate::ThreadHandedness::Right => (*k, p),
        crate::ThreadHandedness::Left => (-*k, -p),
    };
    Ok(kernel::sweep_helix_mesh(&plane, &section, base, sweep_axis, sweep_pitch, (s1 - s0) / p, stations as usize)?)
}

#[cfg(test)]
mod thread_handedness_tests {
    use super::*;

    #[test]
    fn thread_handedness_groove_reflects_exactly_about_the_physical_axis() {
        for direction in [Vec3::Z, Vec3::X, Vec3::new(1.0, 2.0, 3.0)] {
            for internal in [false, true] {
                let axis = UnitVec3::new(direction).unwrap();
                let radius = if internal { 2.5 } else { 3.0 };
                let mut info = ThreadInfo {
                    feature: 1,
                    body: "Stock".into(),
                    designation: "M6x1".into(),
                    major_diameter: 6.0,
                    radius,
                    pitch: 1.0,
                    internal,
                    handedness: crate::ThreadHandedness::Right,
                    axis_point: Vec3::new(7.0, -5.0, 2.0),
                    axis,
                    start: 1.3,
                    end: 4.3,
                };
                let right = thread_groove(&info, radius, (false, true)).unwrap();
                info.handedness = crate::ThreadHandedness::Left;
                let left = thread_groove(&info, radius, (false, true)).unwrap();
                // Reflection in the radial plane of the initial profile, independent of the
                // helix parameterization: the projected coordinate along the axis stays fixed.
                let seed = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
                let normal = (seed - *axis * axis.dot(seed)).normalized().unwrap();
                assert_eq!(right.positions.len(), left.positions.len());
                for (r, l) in right.positions.iter().zip(&left.positions) {
                    let expected = *r - normal * (2.0 * (*r - info.axis_point).dot(normal));
                    assert!((*l - expected).len() < 1e-10, "physical reflection: {direction:?}/{internal}");
                    assert!(((*r - *l).dot(*axis)).abs() < 1e-10, "axial extent must not change");
                }
                let (r, l) = (right.measure(), left.measure());
                assert!(r.volume > 0.0 && l.volume > 0.0, "outward, watertight sweep");
                assert!((r.volume - l.volume).abs() < r.volume * 1e-12, "exact tool volume: {r:?}/{l:?}");
            }
        }
    }
}

/// What a modelled external thread turns off a shaft thicker than its major diameter before it
/// cuts, as a lathe would: the ring between the major diameter and the shaft over the threaded
/// length, run on past an end of the face that nothing of the part (`mb`) lies beyond. `None`
/// for a shaft no thicker than the thread. Its inner wall is the turned face, named `turned`;
/// an end that stops inside the face leaves a shoulder, `shoulder:start` or `shoulder:end`.
fn thread_turn(t: &ThreadInfo, cyl: &kernel::CylinderFace, mb: &ModelBody) -> Result<Option<Traced>> {
    let (p, r) = (t.pitch, t.major_diameter / 2.0);
    if t.internal || !(cyl.radius > r * (1.0 + 1e-6)) {
        return Ok(None);
    }
    let k = t.axis;
    let u = k.cross(if k.x.abs() < 0.9 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) });
    let u = u.normalized().ok_or_else(|| DocError::Invalid("thread axis".into()))?;
    let v = k.cross(u);
    let mesh = mb.mesh();
    let solid_round = |s: f64, radius: f64| {
        (0..12).any(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 12.0;
            mesh.contains(t.axis_point + *k * s + (u * a.cos() + v * a.sin()) * radius)
        })
    };
    // The ring reaches a little past the shaft; the part must not go on there (a shaft standing
    // in a corner, a rounded edge), or the turning would cut into it.
    let out = cyl.radius + 0.1 * p;
    if [0.25, 0.5, 0.75].iter().any(|f| solid_round(t.start + (t.end - t.start) * f, cyl.radius + 0.05 * p)) {
        return Err(DocError::Invalid(format!(
            "this {} mm shaft is too thick for a modelled {} thread, and the part around it keeps it from being turned down; make the shaft {} mm",
            show_mm(2.0 * cyl.radius),
            t.designation,
            show_mm(t.major_diameter)
        )));
    }
    // Past a free end of the face out into the air; elsewhere square at the threaded length's end.
    let tol = p * 1e-3;
    let mid = (r + cyl.radius) / 2.0;
    let free = |at: f64, s: f64, past: f64| (at - s).abs() < tol && !solid_round(s + past * 0.5 * p, mid);
    let (open0, open1) = (free(cyl.start, t.start, -1.0), free(cyl.end, t.end, 1.0));
    let s0 = if open0 { t.start - p } else { t.start };
    let s1 = if open1 { t.end + p } else { t.end };
    let plane = Plane { origin: t.axis_point, x: u, y: *k };
    let ring = [Vec2::new(r, s0), Vec2::new(out, s0), Vec2::new(out, s1), Vec2::new(r, s1)];
    let region = Region2 { outer: solidcraft_geom::Loop2::polygon(&ring), holes: vec![] };
    let ring = kernel::revolve(&plane, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?
        .into_iter()
        .next()
        .ok_or_else(|| DocError::Invalid("thread: the shaft turned down to the thread".into()))?;
    // The ring's sides by profile segment: its start end, outside, end end, inner wall.
    let role = |segment: usize| -> Vec<String> {
        match segment {
            0 if !open0 => vec!["shoulder:start".into()],
            2 if !open1 => vec!["shoulder:end".into()],
            3 => vec!["turned".into()],
            _ => Vec::new(),
        }
    };
    let roles = ring.history().filter(|h| h.operands == 0).map(|h| {
        h.faces
            .iter()
            .map(|f| match f.origins.as_slice() {
                [kernel::history::FaceOrigin::Profile { segment }] => role(*segment),
                _ => Vec::new(),
            })
            .collect()
    });
    Ok(Some(Traced::made(ring, roles)))
}

/// A constant fillet of `edges`; with [`FilletFit::Reduce`](crate::FilletFit::Reduce), a radius too
/// large for the geometry around them is reduced where it doesn't fit, to the largest that works
/// for each group of edges sharing a face; the radius per edge is returned. With
/// [`FilletFit::Taper`](crate::FilletFit::Taper) it tapers along each edge it doesn't fit.
fn fillet_fitted(
    body: &kernel::Body,
    edges: &[Vec3],
    r: f64,
    fit: crate::FilletFit,
) -> std::result::Result<(kernel::Body, Option<Vec<f64>>), kernel::KernelError> {
    match kernel::fillet(body, edges, r) {
        Err(kernel::KernelError::TooLarge(why)) if fit == crate::FilletFit::Reduce => match kernel::fillet_max_each(body, edges, r) {
            // Built at the radius asked for after all (the groups apart): nothing reduced.
            Some((used, b)) => Ok((b, used.iter().any(|u| *u < r).then_some(used))),
            None => Err(kernel::KernelError::TooLarge(why)),
        },
        Err(kernel::KernelError::TooLarge(_)) if fit == crate::FilletFit::Taper => kernel::fillet_tapered(body, edges, r).map(|(b, _)| (b, None)),
        other => other.map(|b| (b, None)),
    }
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

/// Model after some prefix of the timeline.
#[derive(Clone, Debug, Default)]
pub struct ModelState {
    /// Frame placements belong to the evaluator. A weak reference avoids a cycle through
    /// cached bodies' naming origins, which retain earlier model states.
    frame_bodies: std::sync::Weak<std::sync::Mutex<FrameBodies>>,
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
    /// Sheet metal bodies (by body name) and their flat patterns.
    pub sheets: Vec<crate::sheet::SheetBody>,
    /// Construction planes, axes and points as evaluated (by feature).
    pub construct: Vec<crate::construct::Construct>,
    /// Evaluation-cache keys of the sketches (by sketch feature id).
    sketch_keys: BTreeMap<u64, u64>,
    /// The edges each fillet or chamfer blended, as found in this evaluation (by feature id):
    /// a pattern of the feature repeats these, not the points picked when it was made, which a
    /// parameter change may have moved off the edges.
    blend_edges: BTreeMap<u64, Vec<Vec3>>,
    /// The radius per edge a fillet used when it reduced a radius too large for some of its edges
    /// (by feature id, see `FilletFit::Reduce`).
    fitted: BTreeMap<u64, Vec<f64>>,
    /// The radius spline of each edge a fillet tapered (a point on the edge with it, by feature
    /// id, see `FilletFit::Taper`).
    tapered: BTreeMap<u64, Vec<(Vec3, kernel::Taper)>>,
}

impl ModelState {
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
    /// the body counter, threads and sheet bodies.
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
        serde_json::to_string(&self.sheets).unwrap_or_default().hash(&mut h);
        h.finish()
    }

    /// After a feature evaluated to this state from `before` with cache key `key`: bodies it
    /// made or changed take that key, sketches it made a digest of their solved geometry (see
    /// [`quantize`]); the rest keep theirs.
    fn stamp(&mut self, before: &Arc<ModelState>, f: &Feature, key: u64, mats: Vec<Mat>) {
        let feature = f.id;
        let depth = before.bodies.iter().filter_map(|b| b.names.depth()).max().unwrap_or(0) + 1;
        let sheets = if f.kind.is_sheet() { self.sheets.clone() } else { Vec::new() };
        let origin = Arc::new(crate::naming::NamingOrigin { feature: f.clone(), before: before.clone(), depth, mats, sheets });
        // Pieces a cut left are named after their faces, not the kernel's shell order (unless
        // the feature was saved when they were named in that order).
        if !f.shell_piece_order {
            crate::lineage::order_lumps(&mut self.bodies, &origin);
        }
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
            let kept = olds.iter().find(|x| x.body.same(&b.body));
            match kept {
                Some(x) => {
                    b.key = x.key;
                    b.names = x.names.clone();
                    b.bytes = if x.bytes > 0 { x.bytes } else { b.body.retained_bytes() };
                }
                None => {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    (key, &b.name).hash(&mut h);
                    b.key = h.finish();
                    if !matches!(f.kind, FeatureKind::BodyTransfer { .. }) {
                        b.names = crate::naming::NameCell::traced(Some(origin.clone()), b.lineage());
                    }
                    // Faces split on purpose stay split through later features (for export).
                    if b.body.split_lines().is_none()
                        && let Some(lines) = olds.first().and_then(|x| x.body.split_lines())
                    {
                        b.body = b.body.clone().with_split_lines(Some(lines.clone()));
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
            quantize(&mut sketch);
            serde_json::to_string(&sketch).unwrap_or_default().hash(&mut h);
            s.report.ok().hash(&mut h);
            self.sketch_keys.insert(feature, h.finish());
        }
    }
}

/// Evaluation results by cache key, shared by a model and its copies (previews, scratch
/// sessions), so a result computed once is reused wherever the same inputs come back.
#[derive(Debug, Default)]
pub struct EvalCache {
    map: std::collections::HashMap<u64, CachedEval>,
    order: std::collections::VecDeque<u64>,
    /// What the stored results cost together.
    bytes: usize,
    /// The byte budget (0: [`CACHE_BYTES`]).
    pub(crate) budget: usize,
    pub hits: usize,
}

#[derive(Clone, Debug)]
struct CachedEval {
    error: Option<String>,
    warning: Option<String>,
    boolean_diagnostics: kernel::BooleanDiagnostics,
    ms: f64,
    state: Arc<ModelState>,
    /// What the state costs, worked out once when it was stored (see
    /// [`ModelState::retained_bytes`]).
    bytes: usize,
}

/// Results kept (oldest dropped first).
const CACHE_ENTRIES: usize = 4096;

/// Bytes of results kept (oldest dropped first). A cached result is as big as the model it
/// holds, so bounding the cache by entry count alone let a long editing session keep every
/// superseded state: the entries nothing else refers to any more are exactly the ones a byte
/// budget drops.
const CACHE_BYTES: usize = 64 << 20;

impl EvalCache {
    /// A cache that keeps `bytes` of results (always at least the last one): a small budget
    /// stands in for a design whose states are large.
    pub fn with_budget(bytes: usize) -> Self {
        EvalCache { budget: bytes, ..EvalCache::default() }
    }

    /// Results stored.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    fn get(&mut self, key: u64) -> Option<CachedEval> {
        let r = self.map.get(&key).cloned();
        if r.is_some() {
            self.hits += 1;
        }
        r
    }
    fn put(&mut self, key: u64, e: CachedEval) {
        let bytes = e.bytes;
        match self.map.insert(key, e) {
            Some(old) => self.bytes = self.bytes.saturating_sub(old.bytes),
            None => self.order.push_back(key),
        }
        self.bytes = self.bytes.saturating_add(bytes);
        // One result is always kept, so a single state larger than the budget cannot empty the
        // cache and force a full recompute on every undo.
        let budget = if self.budget == 0 { CACHE_BYTES } else { self.budget };
        while self.order.len() > CACHE_ENTRIES || (self.bytes > budget && self.order.len() > 1) {
            if let Some(k) = self.order.pop_front()
                && let Some(old) = self.map.remove(&k)
            {
                self.bytes = self.bytes.saturating_sub(old.bytes);
            }
        }
    }
}

/// The results an evaluation ahead on a copy of a model computed or took from the cache, kept for
/// the run that follows on the model itself (#1989). The evaluation cache is bounded by bytes and
/// charges every entry its whole model, so on a design with large bodies it holds only the last
/// few states: by the time the run came, the cache had dropped most of what was computed ahead,
/// and the run computed it all again, on the window thread.
#[derive(Debug, Default)]
pub struct KeptResults(std::sync::Mutex<std::collections::HashMap<u64, CachedEval>>);

impl KeptResults {
    /// Results kept.
    pub fn len(&self) -> usize {
        self.0.lock().map_or(0, |m| m.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn get(&self, key: u64) -> Option<CachedEval> {
        self.0.lock().ok().and_then(|m| m.get(&key).cloned())
    }

    fn put(&self, key: u64, e: &CachedEval) {
        if let Ok(mut m) = self.0.lock()
            && (m.len() < CACHE_ENTRIES || m.contains_key(&key))
        {
            m.insert(key, e.clone());
        }
    }
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
        bodies + sketches + self.sheets.len() * 4096 + self.threads.len() * 192 + self.construct.len() * 192
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
        bodies + sketches + self.sheets.len() * 4096 + self.threads.len() * 192 + self.construct.len() * 192
    }

    pub fn body(&self, name: &str) -> Option<&ModelBody> {
        #[cfg(test)]
        cost_tests::count(|c| c.body_searches += 1);
        self.bodies.iter().find(|b| b.name == name)
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
    pub warning: Option<String>,
    pub boolean_diagnostics: kernel::BooleanDiagnostics,
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
    /// The imported texts' digests the last `evaluate` fingerprinted the features with.
    step_digests: StepDigests,
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
    /// Results another copy of this model evaluated (ahead, off the window thread) that
    /// `evaluate` takes for features they match ([`Model::prime`]), with that copy's context.
    primed: Option<(u64, Arc<std::collections::HashMap<u64, FeatureResult>>)>,
    /// Shared with copies of this evaluator; states hold only a weak reference.
    frame_bodies: Arc<std::sync::Mutex<FrameBodies>>,
    /// Inputs proven to describe placement-independent geometry, and the state they produced.
    placement_inputs: Option<placement_reuse::Inputs>,
    /// All evaluated features use supported local inputs; computed with the existing owner index.
    placement_local: bool,
}

/// Parameters a feature uses directly (inputs, sketch dimensions and the expressions of a
/// sketch's texts, the text parameters of their words included), sorted.
fn param_names(f: &Feature) -> Vec<String> {
    let mut names: Vec<String> = f.kind.expressions().iter().flat_map(|e| expr::references(e)).collect();
    if let FeatureKind::Sketch { sketch, .. } = &f.kind {
        names.extend(sketch.constraints.iter().filter_map(|c| c.param.clone()));
        names.extend(sketch.patterns.iter().flat_map(|p| p.expressions.values()).flat_map(|e| expr::references(e)));
        names.extend(sketch.text_exprs().into_iter().flat_map(|(_, e)| expr::references(e)));
        names.extend(sketch.text_word_exprs().into_iter().flat_map(|e| crate::text_params::references(e)));
    }
    names.sort();
    names.dedup();
    names
}

fn fingerprint(doc: &Document, prev: u64, f: &Feature, vals: &BTreeMap<String, Value>, rolled_back: bool, steps: &StepDigests) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    prev.hash(&mut h);
    rolled_back.hash(&mut h);
    vals.get(DESIGN_LEN).map(|v| v.v.to_bits()).hash(&mut h);
    hash_feature_json(f, steps, &mut h);
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

/// The digests of the Import features' STEP texts, by feature id, with the text each was taken
/// from. Every evaluation fingerprints every feature, and writing out a design's imported files
/// each time made every command as slow as the files are large: a text is shared (`Arc`), so
/// its digest is taken once and kept while the feature holds that same text.
#[derive(Clone, Debug, Default)]
struct StepDigests(std::collections::HashMap<u64, (Arc<str>, u64)>);

impl StepDigests {
    /// The digests of `doc`'s imported texts, reusing `known` for texts it has seen.
    fn of(doc: &Document, known: &StepDigests) -> StepDigests {
        let texts = doc.features.iter().filter_map(|f| match &f.kind {
            FeatureKind::Import { step, .. } => Some((f.id, step)),
            _ => None,
        });
        StepDigests(
            texts
                .map(|(id, step)| {
                    let digest = known.get(id, step).unwrap_or_else(|| step_text_digest(step));
                    (id, (step.clone(), digest))
                })
                .collect(),
        )
    }

    /// The digest of `step`, when it is the text feature `id` was last seen with.
    fn get(&self, id: u64, step: &Arc<str>) -> Option<u64> {
        self.0.get(&id).filter(|(text, _)| Arc::ptr_eq(text, step)).map(|(_, digest)| *digest)
    }
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

/// A feature as fingerprinted: its definition, a sketch's stored solve quantized, an imported
/// file's text by its digest.
/// Hash [`feature_json`] of `f` into `h`. Every evaluation fingerprints every feature, and in a
/// design built command by command almost all of them are the same as last time: the text of a
/// feature is kept (by id) with the feature it was written from, and written again only when the
/// feature differs. An Import's text also depends on its STEP digest, so it is written each time
/// as before.
///
/// The texts are shared by every thread: a command evaluated ahead runs on a new worker thread
/// each time (the desktop's deferred commands and delete checks, the control channel's ahead
/// jobs), and a per-thread store started empty there, so every ahead run wrote out every
/// feature again. The lock is held only to look up or store an entry, never while a feature is
/// compared or written; an entry is the same whichever thread wrote it, since the text is a
/// function of the feature alone.
fn hash_feature_json(f: &Feature, steps: &StepDigests, h: &mut impl Hasher) {
    type Written = std::collections::HashMap<u64, Arc<(Feature, String)>>;
    static WRITTEN: std::sync::LazyLock<std::sync::Mutex<Written>> = std::sync::LazyLock::new(Default::default);
    if matches!(f.kind, FeatureKind::Import { .. }) {
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

fn feature_json(f: &Feature, steps: &StepDigests) -> String {
    if let FeatureKind::Import { step, .. } = &f.kind {
        let mut g = f.clone();
        if let FeatureKind::Import { step: text, .. } = &mut g.kind {
            let digest = steps.get(f.id, step).unwrap_or_else(|| step_text_digest(step));
            *text = Arc::from(format!("{digest:016x}:{}", step.len()));
        }
        return serde_json::to_string(&g).unwrap_or_default();
    }
    if let FeatureKind::Sketch { .. } = &f.kind {
        let mut g = f.clone();
        if let FeatureKind::Sketch { sketch, .. } = &mut g.kind {
            quantize(sketch);
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
    }
    true
}

impl Document {
    /// The values features are evaluated with: the parameters, and the design's length unit.
    pub fn evaluation_values(&self) -> BTreeMap<String, Value> {
        evaluation_values(self).0
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
    steps: &StepDigests,
    st: Option<&ModelState>,
) -> u64 {
    let mut fp = fingerprint(doc, prev, f, vals, rolled_back, steps) ^ placement_digest(doc, f, st);
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
    // A pattern or mirror of features replays its sources' definitions wherever they are in the
    // timeline. One made with the timeline rolled back comes before them, where the chain doesn't
    // cover them: its copies kept the size the sources had when they were made.
    if let FeatureKind::Pattern { features, bodies, .. } | FeatureKind::Mirror { features, bodies, .. } = &f.kind
        && bodies.is_empty()
        && !features.is_empty()
    {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        fp.hash(&mut h);
        for g in replayed_sources(doc, features) {
            fingerprint(doc, 0, g, vals, false, steps).hash(&mut h);
        }
        fp = h.finish();
    }
    fp
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
        if let FeatureKind::Pattern { features, bodies, .. } | FeatureKind::Mirror { features, bodies, .. } = &g.kind
            && bodies.is_empty()
        {
            todo.extend(features.iter().copied());
        }
        out.push(g);
    }
    out
}

/// Document-level inputs any feature may read: construction planes (resolved by name), sheet
/// metal and plastic rules (and the parameters they use), components and body placement.
fn context_digest(doc: &Document, vals: &BTreeMap<String, Value>) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for f in doc.features.iter().filter(|f| {
        matches!(f.kind, FeatureKind::ConstructionPlane { .. } | FeatureKind::ConstructionAxis { .. } | FeatureKind::ConstructionPoint { .. })
    }) {
        fingerprint(doc, 0, f, vals, false, &StepDigests::default()).hash(&mut h);
    }
    doc.units.hash(&mut h);
    serde_json::to_string(&doc.sheet).unwrap_or_default().hash(&mut h);
    serde_json::to_string(&doc.plastic).unwrap_or_default().hash(&mut h);
    serde_json::to_string(&doc.components).unwrap_or_default().hash(&mut h);
    serde_json::to_string(&doc.body_components).unwrap_or_default().hash(&mut h);
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
            self.all = true;
        }
    }

    /// The same target selection as `apply_traced`, expanded only over the timeline input.
    fn drawn_targets(&mut self, st: Option<&ModelState>, f: &Feature) {
        let Some(st) = st else {
            self.all = true;
            return;
        };
        let (_, targets) = feature_op(f);
        let names = if targets.is_empty() { &f.participants } else { &targets };
        let all = names.len() == 1 && names.first().is_some_and(|n| n == "*");
        if all || names.is_empty() {
            self.bodies.extend(st.bodies.iter().filter(|b| all || b.component == f.component || b.feature == f.id).map(|b| b.name.clone()));
        } else {
            for name in names {
                self.placed_body(Some(st), name);
            }
        }
    }

    /// The component of body `name` in `st`; every component when the body is not there (a
    /// projection then takes the body nearest its point, wherever that is).
    fn body(&mut self, doc: &Document, st: Option<&ModelState>, name: &str) {
        match st.and_then(|st| st.body(name)) {
            Some(b) => {
                self.components.insert(doc.model_body_component(b));
            }
            None => self.all = true,
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
            L::Origin | L::Axis { .. } | L::SketchAxis { .. } | L::Plane { .. } | L::Text { .. } => {}
        }
    }
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

/// Where the geometry a feature reads from other components is placed, as its own component
/// sees it: the placement of each component (or occurrence path) it reads relative to its own
/// frame. A feature that reads nothing outside its own frame gives 0, so moving parts around
/// (a joint driven, an occurrence moved) leaves it as it was; one that reads a part moved
/// relative to it (a sketch projecting a face of another part, To Object, a mirror of a placed
/// body) is evaluated again. `st` is the model the feature starts from, which says what
/// component a body it names belongs to (without it, every component is taken as read).
/// [`placement_digest`] of a mirror of placed bodies: its own component's frame, each placement it
/// was picked at (the copy's component, frame and the frame it was picked in) and the offsets of
/// the bodies it mirrors (#1938).
fn mirror_placements_digest(doc: &Document, f: &Feature, placements: &[Vec<u64>], bodies: &[crate::BodyName]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    doc.component_transform(f.component).iter().flatten().for_each(|x| x.to_bits().hash(&mut h));
    for placed in placements.iter().map(|path| mirror_copy_frame(doc, path)) {
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

fn placement_digest(doc: &Document, f: &Feature, st: Option<&ModelState>) -> u64 {
    // No shortcut for a design without placements: its first component (placed where the
    // design is) must not change what its features read, or creating it evaluates them all.
    let mut reads = PlacementReads::default();
    let mut frame = None;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // A sketch, profile or path of another component is moved through both placements.
    for (sketch, _) in f.kind.profile_sels() {
        if doc.feature(sketch).is_some_and(|s| s.component != f.component) {
            reads.sketch(doc, sketch);
        }
    }
    let path = match &f.kind {
        FeatureKind::Sweep { path_sketch, .. }
        | FeatureKind::SurfaceSweep { path_sketch, .. }
        | FeatureKind::Pipe { path_sketch, .. }
        | FeatureKind::FrameMember { path_sketch, .. }
        | FeatureKind::Pattern { pattern: crate::PatternKind::Path { path_sketch, .. }, .. } => Some(*path_sketch),
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
    if path.as_ref().is_some_and(|p| member || wired(p)) || rails.iter().any(|r| wired(&r.sketch)) {
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
    match &f.kind {
        // To / From Object finds its target where that target's component is placed (#928),
        // over every body.
        FeatureKind::Extrude { extent, .. } if extent.to.is_some() || extent.from.is_some() => reads.all = true,
        FeatureKind::Revolve { to: Some(_), .. } => reads.all = true,
        // A mirror of placed bodies follows the placements it was picked at and its own component's
        // frame, not every occurrence: moving an unrelated one leaves it (and its cache) alone.
        FeatureKind::Mirror { placements, bodies, .. } if !placements.is_empty() => return mirror_placements_digest(doc, f, placements, bodies),
        // A combine moves tools placed differently from its target into the target's frame (#3383).
        FeatureKind::Combine { target, tools, .. } => {
            frame = st.and_then(|st| st.body(target)).map(|b| body_world(doc, b));
            reads.placed_body(st, target);
            for tool in tools {
                reads.placed_body(st, tool);
            }
        }
        // A pattern of bodies copies them along its directions or about its axis in its own
        // component's frame, so it follows where each body it copies is placed relative to that
        // frame: copies of a root block, patterned in a component, move with the component.
        FeatureKind::Pattern { bodies, .. } if !bodies.is_empty() => {
            for b in bodies {
                reads.placed_body(st, &b.0);
            }
        }
        // Joins, cuts and intersects meet their targets where those are placed (#3383).
        kind if drawn_tools_change_bodies(kind) => reads.drawn_targets(st, f),
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
        _ => {}
    }
    if reads.is_empty() {
        return 0;
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

    /// The final model state.
    pub fn state(&self) -> Arc<ModelState> {
        self.results.iter().rev().find(|r| !r.skipped).map(|r| r.state.clone()).unwrap_or_else(|| self.empty.clone())
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
        let steps = StepDigests::of(doc, &self.step_digests);
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
        let steps = StepDigests::of(doc, &self.step_digests);
        let marker = doc.marker.unwrap_or(usize::MAX);
        let mut prev = 0;
        // The model each feature starts from (which components the bodies it names are in).
        let mut state = self.empty.clone();
        doc.features.iter().zip(&self.results).enumerate().all(|(i, (f, r))| {
            prev = chain_fingerprint(doc, &vals, f, prev, i >= marker, &steps, Some(&state));
            if !r.skipped {
                state = r.state.clone();
            }
            r.id == f.id && r.fingerprint == prev
        })
    }

    /// Take the results `ahead` (a copy of this model, evaluated off this thread for the same
    /// commands) computed: until [`Model::unprime`], `evaluate` reuses each one whose feature
    /// has the same inputs here, instead of evaluating it again. Results that don't match are
    /// ignored, so a design changed meanwhile is evaluated as usual.
    pub fn prime(&mut self, ahead: &Model) {
        let results = ahead.results.iter().filter(|r| !r.skipped && !r.stale).map(|r| (r.id, r.clone())).collect();
        self.primed = Some((ahead.context, Arc::new(results)));
    }

    /// Stop offering the results [`Model::prime`] took.
    pub fn unprime(&mut self) {
        self.primed = None;
    }

    /// Re-evaluate the document, reusing results up to the first changed feature.
    pub fn evaluate(&mut self, doc: &Document) {
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
        let ctx = context_digest(doc, &vals);
        let previous_context = self.context;
        let mut rolled_results = std::mem::take(&mut self.rolled_results);
        self.context = ctx;
        self.step_digests = StepDigests::of(doc, &self.step_digests);
        let primed = self.primed.as_ref().filter(|(context, _)| !self.no_cache && *context == ctx).map(|(_, results)| results.clone());
        // Each feature's component, for `Document::legacy_body_component` on every body after
        // every feature without searching the timeline for each (the first feature with an id).
        let mut feature_component: std::collections::HashMap<u64, u64> = std::collections::HashMap::with_capacity(doc.features.len());
        for f in &doc.features {
            feature_component.entry(f.id).or_insert(f.component);
        }
        let watched = crate::progress::begin(doc.features.len());
        // The features that failed so far in this pass, by id: their names and errors (#9075).
        let mut failed: std::collections::HashMap<u64, (String, String)> = std::collections::HashMap::new();
        for (i, f) in doc.features.iter().enumerate() {
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
            let fp = chain_fingerprint(doc, &vals, f, prev_fp, rolled_back, &self.step_digests, Some(&state));
            prev_fp = fp;
            let current = |old: &&FeatureResult| old.id == f.id && old.fingerprint == fp && !old.stale;
            // A state the saved model did not keep is evaluated again from the one before it; the
            // results after it that the model did keep still stand (#3220).
            let unkept = reuse && self.results.get(i).filter(current).is_some_and(|old| old.unkept);
            if f.suppressed && !rolled_back {
                reserved.extend(f.body_names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()).map(str::to_string));
            }
            if reuse
                && !unkept
                && let Some(old) = self.results.get(i).filter(current)
            {
                if !old.skipped {
                    state = old.state.clone();
                }
                out.push(old.clone());
                continue;
            }
            reuse = unkept && !cancelled;
            if f.suppressed || rolled_back || cancelled {
                out.push(FeatureResult {
                    id: f.id,
                    name: f.name.clone(),
                    error: None,
                    warning: None,
                    boolean_diagnostics: Default::default(),
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
                    warning: None,
                    boolean_diagnostics: Default::default(),
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
            let key =
                cache_key(f, fp, fingerprint(doc, 0, f, &vals, false, &self.step_digests) ^ placement_digest(doc, f, Some(&state)), ctx, &state);
            let kept = self.kept.0.as_ref().filter(|_| !self.no_cache);
            if !self.no_cache
                && let Some(c) = self.cache.lock().ok().and_then(|mut m| m.get(key)).or_else(|| kept.and_then(|k| k.get(key)))
            {
                if let Some(k) = kept {
                    k.put(key, &c);
                }
                restored += 1;
                if c.error.is_none() {
                    state = c.state.clone();
                }
                // A kept result after a failed one would not be this evaluation's.
                reuse &= c.error.is_none();
                out.push(FeatureResult {
                    id: f.id,
                    name: f.name.clone(),
                    error: c.error,
                    warning: c.warning,
                    boolean_diagnostics: c.boolean_diagnostics,
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
            if let Some(r) = rolled.as_ref().filter(current).or_else(|| primed.as_ref().and_then(|p| p.get(&f.id)).filter(current)) {
                restored += 1;
                if r.error.is_none() {
                    state = r.state.clone();
                }
                reuse &= r.error.is_none();
                out.push(FeatureResult { state: state.clone(), unkept: false, ..r.clone() });
                continue;
            }
            recomputed += 1;
            let before = state.clone();
            let t0 = now();
            let mut next = (*state).clone();
            // The names the design must not use again travel with the state (#4957, #4302).
            next.reserved_body_names.clone_from(&reserved);
            next.frame_bodies = Arc::downgrade(&self.frame_bodies);
            for b in &mut next.bodies {
                b.made = crate::lineage::Made::Before;
            }
            let mut warning = None;
            // Face references by name: move each face point onto its named face (on the body the
            // feature names, when it names one and the face is there).
            let renamed;
            let f = match with_named_face_points(doc, &state, f) {
                Some((g, notes)) => {
                    if !notes.is_empty() {
                        warning = Some(notes.join("; "));
                    }
                    renamed = g;
                    &renamed
                }
                None => f,
            };
            // A parameter with an error fails the features using it, with that error.
            let bad_param = param_names(f).into_iter().find_map(|n| self.param_errors.get(&n).map(|e| format!("parameter `{n}`: {e}")));
            let (r, mut boolean_diagnostics) = kernel::capture_boolean_diagnostics(|| match bad_param {
                Some(e) => Err(kernel::KernelError::Failed(e)),
                None => kernel::guard(&f.name, || {
                    eval_feature(doc, &vals, f, &mut next, &mut warning).map_err(|e| kernel::KernelError::Failed(e.to_string()))
                }),
            });
            if r.is_ok() {
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
                if let Some(note) = boolean_diagnostics.warning() {
                    append_warning(&mut warning, note);
                }
                track_altered_sheets(&state, &mut next, f, &mut warning);
                note_join_apart(&state, &next, f, &mut warning);
            } else {
                boolean_diagnostics = Default::default();
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0;
            let error = match r {
                Ok(()) => {
                    // Copies are named after their sources through the instance transforms.
                    let mats = match &f.kind {
                        FeatureKind::Pattern { pattern, .. } => pattern_transforms(doc, &vals, &state, f.component, pattern).unwrap_or_default(),
                        FeatureKind::Mirror { plane, .. } => {
                            doc.resolve_plane_in(&vals, plane, 0, Some(&state)).map(|pl| vec![mirror_matrix(&pl)]).unwrap_or_default()
                        }
                        _ => Vec::new(),
                    };
                    // Copies made through a pattern or mirror among the sources come after (#395).
                    let mats = match &f.kind {
                        FeatureKind::Pattern { features, bodies, .. } | FeatureKind::Mirror { features, bodies, .. } if bodies.is_empty() => {
                            copy_instances(doc, &vals, &state, features, &mats).unwrap_or(mats)
                        }
                        _ => mats,
                    };
                    next.stamp(&state, f, key, mats);
                    for b in &mut next.bodies {
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
            if !self.no_cache && !watched.cancelled() {
                let e = CachedEval {
                    error: error.clone(),
                    warning: warning.clone(),
                    boolean_diagnostics: boolean_diagnostics.clone(),
                    ms,
                    bytes: state.added_bytes(&before),
                    state: state.clone(),
                };
                if let Some(k) = &self.kept.0 {
                    k.put(key, &e);
                }
                if let Ok(mut m) = self.cache.lock() {
                    m.put(key, e);
                }
            }
            reuse &= error.is_none();
            out.push(FeatureResult {
                id: f.id,
                name: f.name.clone(),
                error,
                warning,
                boolean_diagnostics,
                ms,
                skipped: false,
                stale: false,
                fingerprint: fp,
                unkept: false,
                state: state.clone(),
            });
        }
        self.placement_local = self.placement_local && out.iter().all(|r| !r.skipped && !r.stale && !r.unkept && r.error.is_none());
        self.results = out;
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
        // no other body's name comes from its name.
        let held: Vec<bool> = self.results.iter().skip(at).map(|r| r.state.body(old).is_some()).collect();
        let one_run = held.iter().skip_while(|h| **h).all(|h| !*h);
        let derived = format!("{old} (");
        let same_body = || {
            self.results
                .iter()
                .skip(at)
                .all(|r| r.state.bodies.iter().all(|b| if b.name == old { b.feature == feature } else { !b.name.starts_with(&derived) }))
        };
        let quoted = self.results.iter().skip(at).any(|r| [&r.error, &r.warning].into_iter().flatten().any(|m| m.contains(old)));
        let named_elsewhere = self
            .results
            .iter()
            .any(|r| r.state.body(new).is_some() || r.state.sheets.iter().any(|x| x.body == old) || r.state.threads.iter().any(|t| t.body == old));
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
                        b.names = b.names.renamed();
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
        self.step_digests = StepDigests::of(doc, &self.step_digests);
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
            prev = chain_fingerprint(doc, &vals, f, prev, i >= marker, &self.step_digests, Some(&before));
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

pub(crate) fn val(vals: &BTreeMap<String, Value>, e: &str, k: Kind) -> Result<f64> {
    let scale = vals.get(DESIGN_LEN).map(|x| x.v).unwrap_or(1.0);
    // Non-literal lengths (a parameter ± a bare number) must read that bare number in the design's
    // units during the expression, not only when the whole result is unit-less (#9015).
    if k == Kind::Length && scale != 1.0 && !expr::is_literal(e) {
        let defaults = expr::Defaults { len: scale, ..expr::Defaults::default() };
        let look = |n: &str| vals.get(n).copied().ok_or_else(|| DocError::Expr(format!("unknown parameter `{n}`")));
        return expr::eval_with_defaults(e, &look, defaults)?.to_kind_in(k, defaults);
    }
    Document::eval_in(vals, e, k)
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

pub fn select_profiles(ss: &SolvedSketch, sel: &ProfileSel) -> Result<Vec<Region2>> {
    let ps = &ss.profiles;
    if ps.is_empty() {
        return Err(DocError::Invalid(format!("{} has no closed profiles", ss.name)));
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

fn new_name(state: &mut ModelState, f: &Feature, k: usize) -> String {
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

/// The k-th body of a body pattern: its stored name, else a copy of `source`. Once `body.rename`
/// has stored this feature's names, a copy beyond them (the count was raised) is numbered after
/// the highest stored `source (n)`, so it never takes a number freed by a rename (#372).
/// Features without stored names keep the first free `source (n)`.
fn pattern_copy_name(state: &ModelState, f: &Feature, k: usize, source: &str) -> String {
    if f.body_names.get(k).is_some_and(|name| !name.trim().is_empty()) {
        return copy_body_name(state, f, k, source);
    }
    let base = if source.trim().is_empty() { "Body" } else { source.trim() };
    let prefix = format!("{base} (");
    let highest = f.body_names.iter().filter_map(|n| n.trim().strip_prefix(prefix.as_str())?.strip_suffix(')')?.parse::<u32>().ok()).max();
    match highest {
        Some(h) => {
            (u64::from(h) + 1..).map(|i| format!("{base} ({i})")).find(|n| state.body(n).is_none()).unwrap_or_else(|| unique_body_name(state, base))
        }
        None => unique_body_name(state, base),
    }
}

fn copy_body_name(state: &ModelState, f: &Feature, k: usize, source: &str) -> String {
    let base = f.body_names.get(k).filter(|name| !name.trim().is_empty()).map(String::as_str).unwrap_or(source);
    unique_body_name(state, base)
}

/// Keeps each sheet metal body's flat pattern true to the body (see `SheetBody::altered_by`).
/// A feature that changes a sheet body's material but not its sheet model (a fillet, a join, a
/// mirror combined into it) marks the sheet with its name, and says so. A feature that rebuilds
/// the body from the model (a sheet metal feature, or machining the model keeps as cut-outs,
/// which changes the model) clears the mark, and says that the change it drops is not kept.
/// Features that move whole bodies or only mark their faces (a cosmetic thread, not a modelled
/// one, which cuts material) leave the flat pattern as it was.
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
    let ModelState { sheets, bodies, .. } = after;
    for sh in sheets.iter_mut() {
        let Some(old) = before.sheets.iter().find(|s| s.body == sh.body) else { continue };
        let (Some(was), Some(now)) = (before.body(&sh.body), bodies.iter().find(|b| b.name == sh.body)) else { continue };
        if was.body.same_shape(&now.body) {
            continue;
        }
        if f.kind.is_sheet() || sh != old {
            if let Some(lost) = sh.altered_by.take() {
                append_warning(
                    warning,
                    format!("`{}` is rebuilt from its sheet metal features: what {lost} changed is not kept (not supported yet)", sh.body),
                );
            }
        } else if sh.altered_by.is_none() {
            sh.altered_by = Some(f.name.clone());
            append_warning(
                warning,
                format!(
                    "`{}` is sheet metal: its flat pattern, DXF and bend table can't include this change yet and are refused; a later sheet metal feature rebuilds the body without it",
                    sh.body
                ),
            );
        }
    }
}

/// A Join whose material meets none of its targets still joins it, to the first target as a
/// separate piece (see `apply_traced`): a valid body in several pieces, but rarely what was meant
/// when Join was left on by mistake. Says so when a feature's Join leaves the bodies it changed
/// in more pieces than they had (#4434). Combine joins the bodies it is given, apart or not.
fn note_join_apart(before: &ModelState, after: &ModelState, f: &Feature, warning: &mut Option<String>) {
    let operation = match &f.kind {
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
        _ => return,
    };
    if operation != Operation::Join {
        return;
    }
    for (name, n) in join_pieces_added(before, after) {
        append_warning(
            warning,
            format!("the joined piece doesn't touch `{name}`, so `{name}` is now in {n} separate pieces (New Body makes it a body of its own)"),
        );
    }
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

/// Add tool bodies to the model according to the operation.
fn sheet_boolean_unchanged(body: &Body, tools: &[Body], op: Operation) -> Result<()> {
    if sheet_boolean_changes(body, tools, op)? {
        return Err(DocError::Invalid("this sheet machining cannot be preserved in its flat material: use a parallel, untapered extrude pocket/cut, or unfold before intersecting; unsupported 3D sheet machining is refused to prevent material restoration".into()));
    }
    Ok(())
}

fn sheet_boolean_changes(body: &Body, tools: &[Body], op: Operation) -> Result<bool> {
    let united;
    let tools: &[Body] = if op == Operation::Intersect {
        united = intersection_tool_union(tools.iter().cloned().map(Traced::unnamed).collect())?.into_iter().map(|t| t.body).collect::<Vec<_>>();
        &united
    } else {
        tools
    };
    for tool in tools {
        let test = if op == Operation::Cut { BoolOp::Intersect } else { BoolOp::Cut };
        if kernel::boolean(body, tool, test)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Selected profiles form one machining tool, including disconnected tool material.
/// Balanced unions avoid repeatedly rebuilding a growing aggregate boundary.
fn intersection_tool_union(tools: Vec<Traced>) -> Result<Vec<Traced>> {
    if tools.len() > 1000 || tools.iter().fold(0usize, |n, t| n.saturating_add(t.body.face_count())) > 16_384 {
        return Err(DocError::Invalid("intersection tools exceed the bounded geometry budget".into()));
    }
    let mut groups = tools;
    while groups.len() > 1 {
        let mut next = Vec::new();
        let mut iter = groups.into_iter();
        while let Some(a) = iter.next() {
            match iter.next() {
                Some(b) => {
                    let u = kernel::boolean(&a.body, &b.body, BoolOp::Union)?
                        .ok_or_else(|| DocError::Invalid("intersection tool union is empty".into()))?;
                    next.push(Traced::step(u, &[&a, &b]));
                }
                None => next.push(a),
            }
        }
        groups = next;
    }
    Ok(groups)
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
    let rest: Vec<String> = st
        .bodies
        .iter()
        .filter(|b| {
            !handled.contains(&b.name)
                && (all || if effective.is_empty() { b.component == f.component || b.feature == f.id } else { effective.contains(&b.name) })
        })
        .map(|b| b.name.clone())
        .collect();
    if rest.is_empty() {
        return Ok(());
    }
    apply_traced(doc, st, f, tools, op, &rest)
}

/// Only one target meets every separated tool. A tool with no contact must retain
/// Join's first-target fallback; tools touching different targets must retain absorption order.
fn common_join_target(state: &ModelState, indices: &[usize], tools: &[Traced]) -> Result<Option<usize>> {
    if indices.len() > 64 {
        return Ok(None);
    }
    let bodies: Vec<_> = tools.iter().map(|tool| tool.body.clone()).collect();
    let mut contacts = vec![None; tools.len()];
    for &index in indices {
        let body = state.bodies.get(index).ok_or_else(|| DocError::Invalid("join target disappeared".into()))?;
        for (contact, meets) in contacts.iter_mut().zip(body.body.meets_each(&bodies)?) {
            if meets {
                if contact.is_some() {
                    return Ok(None);
                }
                *contact = Some(index);
            }
        }
    }
    let common = contacts.first().copied().flatten();
    Ok(common.filter(|index| contacts.iter().all(|contact| *contact == Some(*index))))
}

fn apply_op(doc: &Document, state: &mut ModelState, f: &Feature, tools: Vec<Body>, op: Operation, targets: &[String]) -> Result<()> {
    apply_traced(doc, state, f, tools.into_iter().map(Traced::unnamed).collect(), op, targets)
}

/// Many cut tools clear of each other (a pattern's copies, the circles of a vent field in one
/// sketch) as one tool of many pieces, so the target is cut once instead of rebuilding an ever
/// more complex body for every tool (#261, #368). Tools that may meet are left as they are.
fn clear_tools_as_one(tools: Vec<Traced>) -> Vec<Traced> {
    if tools.len() < 8 {
        return tools;
    }
    let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
    match kernel::union_apart(&bodies) {
        // Each piece's faces keep the names of the tool it is.
        Ok(Some(one)) => vec![Traced::step(one, &tools.iter().collect::<Vec<_>>())],
        _ => tools,
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

/// The bodies a feature's boolean takes part in, and whether they were named rather than taken
/// by default (every body, or the feature's component).
pub(crate) fn boolean_scope(state: &ModelState, f: &Feature, targets: &[String]) -> (bool, Vec<String>) {
    let effective = if targets.is_empty() { &f.participants } else { targets };
    let all = effective.len() == 1 && effective.first().is_some_and(|t| t == "*");
    let names = state
        .bodies
        .iter()
        .filter(|b| all || if effective.is_empty() { b.component == f.component || b.feature == f.id } else { effective.contains(&b.name) })
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
    let tools = if op == Operation::Intersect { intersection_tool_union(tools)? } else { tools };
    // Participants: the feature's own targets, else its explicit participants, else the bodies
    // of its component (and what it made itself): other components are left alone.
    let targets: &[String] = if targets.is_empty() { &f.participants } else { targets };
    let all = targets.len() == 1 && targets.first().is_some_and(|t| t == "*");
    let named = !all && !targets.is_empty();
    let mut target_idx: Vec<usize> = if all {
        (0..state.bodies.len()).collect()
    } else if targets.is_empty() {
        (0..state.bodies.len()).filter(|i| state.bodies.get(*i).is_some_and(|b| b.component == f.component || b.feature == f.id)).collect()
    } else {
        let mut v = Vec::new();
        for t in targets {
            v.push(state.bodies.iter().position(|b| &b.name == t).ok_or_else(|| DocError::Unknown(format!("body `{t}`")))?);
        }
        v
    };
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
    if matches!(op, Operation::Cut | Operation::Intersect) {
        for &i in &target_idx {
            if let Some(mb) = state.bodies.get(i).filter(|b| state.sheets.iter().any(|s| s.body == b.name)) {
                let m = tool_frame(&mb.name);
                let bodies: Vec<Body> = tools.iter().map(|t| tool_in(t, m).map(|t| t.body)).collect::<Result<_>>()?;
                sheet_boolean_unchanged(&mb.body, &bodies, op)?;
            }
        }
    }
    // Batching solid targets must preserve Join's absorption order,
    // sheet material tracking and the separate bodies of a New Body extrusion.
    if tools.len() > 1
        && matches!(f.kind, FeatureKind::Extrude { .. })
        && matches!(op, Operation::Join | Operation::Cut)
        && !target_idx.is_empty()
        && target_idx.iter().all(|i| state.bodies.get(*i).is_some_and(|b| !b.body.is_mesh() && !state.sheets.iter().any(|s| s.body == b.name)))
        && let Ok(Some(compound)) = kernel::compound_disjoint_extrusions(&tools.iter().map(|tool| tool.body.clone()).collect::<Vec<_>>())
    {
        let target = if target_idx.len() == 1 {
            target_idx.first().copied()
        } else if op == Operation::Join {
            common_join_target(state, &target_idx, &tools).ok().flatten()
        } else {
            None
        };
        // The optimization must not reject geometry the original sequential path can
        // handle. A failed batch leaves the model untouched and retries the original tools.
        let batch_targets = if op == Operation::Cut {
            // Subtracting a union equals subtracting its separated tools in order.
            // Keep ALL original participants; no sampled contact test excludes a body.
            Some(target_idx.iter().filter_map(|index| state.bodies.get(*index).map(|body| body.name.clone())).collect::<Vec<_>>())
        } else {
            target.and_then(|index| state.bodies.get(index)).map(|body| vec![body.name.clone()])
        };
        if let Some(names) = batch_targets {
            let mut batched = state.clone();
            // Preserve the original participants. For Join, the compound's larger
            // size must not expand the contact tolerance and absorb a nearby body.
            if apply_traced(doc, &mut batched, f, vec![Traced::step(compound, &tools.iter().collect::<Vec<_>>())], op, &names).is_ok() {
                *state = batched;
                return Ok(());
            }
        }
    }
    match op {
        Operation::Join => {
            // Each tool joins every target it meets into one body, kept under the first one's
            // name; a tool that meets none joins the first target as a separate lump. A sheet
            // metal body keeps its own identity: only the first body met may be one.
            let mut live: Vec<String> = target_idx.iter().filter_map(|i| state.bodies.get(*i).map(|b| b.name.clone())).collect();
            // Each union keeps the lineage of what it joins, so the joined body's faces keep
            // their names (the tool's new faces are named by their roles).
            let union = |a: &Traced, b: &Traced| -> Result<Traced> {
                let u = kernel::boolean(&a.body, &b.body, BoolOp::Union)?.ok_or_else(|| DocError::Invalid("join produced nothing".into()))?;
                Ok(Traced::step(u, &[a, b]))
            };
            for tool in &tools {
                let mut met: Vec<String> = Vec::new();
                for name in &live {
                    let Some(mb) = state.body(name) else { continue };
                    let sheet = state.sheets.iter().any(|s| &s.body == name);
                    if mb.body.is_mesh() || (!met.is_empty() && sheet) {
                        continue;
                    }
                    // Each body is met where it is shown: the tool goes into its frame (#3383).
                    if mb.body.meets(&tool_in(tool, tool_frame(name))?.body)? {
                        met.push(name.clone());
                    }
                }
                let Some(keep) = met.first().or(live.first()).cloned() else { return Ok(()) };
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
                    *mb = ModelBody::traced_new(mb.name.clone(), acc, mb.feature);
                }
                state.bodies.retain(|b| !absorbed.contains(&b.name));
                for t in state.threads.iter_mut().filter(|t| absorbed.contains(&t.body)) {
                    t.body = keep.clone();
                }
                live.retain(|n| !absorbed.contains(n));
            }
        }
        Operation::Cut | Operation::Intersect => {
            let bop = if op == Operation::Cut { BoolOp::Cut } else { BoolOp::Intersect };
            let tools = if op == Operation::Cut { clear_tools_as_one(tools) } else { tools };
            // Each target's pieces before the cut: a body joined from separate pieces stays one.
            let affected: std::collections::BTreeMap<String, usize> =
                target_idx.iter().filter_map(|i| state.bodies.get(*i)).map(|b| (b.name.clone(), b.body.lumps().map_or(1, |l| l.len()))).collect();
            let mut remove = Vec::new();
            for &ti in &target_idx {
                let Some(mut acc) = state.bodies.get(ti).map(|b| Some(b.traced())) else { continue };
                // The tools meet the target where it is shown: in its frame (#3383).
                let into = state.bodies.get(ti).and_then(|b| tool_frame(&b.name));
                for t in &tools {
                    let t = &tool_in(t, into)?;
                    acc = match acc {
                        Some(a) => kernel::boolean(&a.body, &t.body, bop)?.map(|r| Traced::step(r, &[&a, t])),
                        None => None,
                    };
                }
                if op == Operation::Intersect && acc.as_ref().is_none_or(|b| b.body.face_count() == 0) {
                    let Some(mb) = state.bodies.get(ti) else { continue };
                    // A tool that meets the body yet leaves nothing is a failed boolean, not an empty result.
                    let mut met = false;
                    if !named && !mb.body.is_mesh() {
                        for t in &tools {
                            if mb.body.meets(&tool_in(t, into)?.body)? {
                                met = true;
                                break;
                            }
                        }
                    }
                    if named || met {
                        return Err(empty_intersection(&[mb.name.as_str()]));
                    }
                    remove.push(ti);
                    continue;
                }
                match acc {
                    Some(b) => {
                        if let Some(mb) = state.bodies.get_mut(ti) {
                            *mb = ModelBody::traced_new(mb.name.clone(), b, mb.feature);
                        }
                    }
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
                let Some(&had) = affected.get(&mb.name).filter(|_| !state.sheets.iter().any(|s| s.body == mb.name)) else {
                    split.push(mb);
                    continue;
                };
                let pieces = mb.body.lumps().unwrap_or_else(|_| vec![mb.body.clone()]);
                if pieces.len() <= had {
                    split.push(mb);
                    continue;
                }
                // Each piece's faces are named after the whole's, worked out once for the whole
                // (so pieces of one face in different bodies are told apart).
                let lineages = crate::lineage::Lineage::lumps(&mb.body, mb.lineage().as_deref(), &pieces);
                for (k, (p, lineage)) in pieces.into_iter().zip(lineages).enumerate() {
                    let (name, made_by) = if k == 0 {
                        (mb.name.clone(), mb.feature)
                    } else {
                        let stored = f.body_names.get(extra).filter(|n| !n.trim().is_empty() && !taken.contains(*n)).cloned();
                        extra += 1;
                        let name = stored.unwrap_or_else(|| {
                            (k..=k + taken.len()).map(|j| format!("{} ({j})", mb.name)).find(|n| !taken.contains(n)).unwrap_or_default()
                        });
                        (name, f.id)
                    };
                    taken.insert(name.clone());
                    split.push(ModelBody::new(name, p, made_by).with_lineage(lineage));
                }
            }
            state.bodies = split;
        }
        Operation::NewBody => {}
    }
    Ok(())
}

/// Distance that reaches through every target body from the sketch plane (either side).
pub fn through_all_distance(st: &ModelState, plane: &Plane, targets: &[String]) -> Result<f64> {
    let mut far: f64 = 0.0;
    let mut any = false;
    for b in st.bodies.iter().filter(|b| targets.is_empty() || targets.contains(&b.name)) {
        let bb = b.mesh().bounds();
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
            far = far.max(plane.height(p).abs());
        }
    }
    if !any {
        return Err(DocError::Invalid("through all needs a body to cut".into()));
    }
    Ok(far * 1.02 + 1.0)
}

/// Sample points inside a region (sketch coordinates).
fn region_samples(r: &Region2) -> Vec<Vec2> {
    let mut out = vec![r.interior_point()];
    let solidcraft_geom::Aabb2 { min: lo, max: hi } = r.outer.polyline(1e-2).bounds();
    for i in 0..5 {
        for j in 0..5 {
            let p = Vec2::new(lo.x + (hi.x - lo.x) * (0.1 + 0.2 * i as f64), lo.y + (hi.y - lo.y) * (0.1 + 0.2 * j as f64));
            if r.contains(p) {
                out.push(p);
            }
        }
    }
    out
}

/// The region with each hole filled whose inside the tool would sweep with no change to the
/// targets: all empty for a cut (the profile drawn round a hole in the part), all material for a
/// join. Then the tool's walls don't lie on the part's walls there (the boolean can't intersect
/// walls lying on each other, #1360), and what it cuts or joins is the same. A hole is filled only
/// when points across it at levels through the tool are all empty (or all material) and no
/// vertex of the targets lies inside the column over it (an island in it, a floor).
fn fill_void_holes(st: &ModelState, r: &Region2, op: Operation, targets: &[String], plane: &Plane, (lo, hi): (f64, f64)) -> Region2 {
    if r.holes.is_empty() || !matches!(op, Operation::Cut | Operation::Join) || hi - lo <= 0.0 {
        return r.clone();
    }
    let bodies: Vec<&ModelBody> = st.bodies.iter().filter(|b| targets.is_empty() || targets.contains(&b.name)).collect();
    if bodies.is_empty() {
        return r.clone();
    }
    let meshes: Vec<Arc<Mesh>> = bodies.iter().map(|b| b.mesh()).collect();
    let size = bodies.iter().map(|b| b.body.size()).fold(0.0, f64::max);
    let margin = (size * 1e-4).max(1e-4);
    let n = plane.normal();
    // Many probes on one mesh each: indexed, the same answers as `Mesh::contains`.
    let inside: Vec<_> = meshes.iter().map(|m| m.inside_index()).collect();
    let inside_any = |p: Vec3| inside.iter().any(|m| m.contains(p));
    let want = op == Operation::Join;
    let mut holes = Vec::with_capacity(r.holes.len());
    for h in &r.holes {
        let column = Region2 { outer: h.ccw(), holes: vec![] };
        let samples = region_samples(&column);
        let swept = (1..=8).all(|k| {
            let z = lo + (hi - lo) * (k as f64 - 0.5) / 8.0;
            samples.iter().all(|s| inside_any(plane.to_world(*s) + n * z) == want)
        });
        // No boundary of the targets within the column (shrunk off its wall, short of its ends).
        let clear = swept
            && meshes.iter().all(|m| {
                m.positions.iter().all(|p| {
                    let z = (*p - plane.origin).dot(n);
                    let q = plane.to_local(*p);
                    !(z > lo + margin
                        && z < hi - margin
                        && column.contains(q)
                        && h.segs.iter().all(|sg| sg.point_at(sg.closest_param(q)).dist(q) > margin))
                })
            });
        if !clear {
            holes.push(h.clone());
        }
    }
    Region2 { outer: r.outer.clone(), holes }
}

/// The bodies a cut or join works on (its targets, else every body), asked what its tool would
/// take or add if it reached past its sketch.
struct Surroundings<'a> {
    /// Each body with its display mesh and, once something near it is asked, its faces.
    bodies: Vec<(&'a ModelBody, Arc<Mesh>, std::cell::OnceCell<Option<kernel::Clearance<'a>>>)>,
    op: Operation,
    /// The largest body's size.
    size: f64,
    /// Faces this close to a plane lie in it (the kernel's measure for coincident faces).
    skin: f64,
    /// How far off a round face a space must stay to be asked about: the face's triangles may
    /// stand off it by twice the chord they are made with, and the check pads them as far again.
    round: f64,
}

impl<'a> Surroundings<'a> {
    /// The chord of the meshes the bodies' faces are asked on, as a share of each one's size.
    const CHORD: f64 = 2e-4;

    /// `None` for an operation other than a cut or a join, or without a body to work on.
    fn of(st: &'a ModelState, op: Operation, targets: &[String]) -> Option<Surroundings<'a>> {
        if !matches!(op, Operation::Cut | Operation::Join) {
            return None;
        }
        let bodies: Vec<_> =
            st.bodies.iter().filter(|b| targets.is_empty() || targets.contains(&b.name)).map(|b| (b, b.mesh(), std::cell::OnceCell::new())).collect();
        let size = bodies.iter().map(|(b, _, _)| b.body.size()).fold(0.0, f64::max);
        (!bodies.is_empty()).then(|| Surroundings { bodies, op, size, skin: (size * 1e-6).max(1e-8), round: 4.0 * Self::CHORD * size })
    }

    /// Does the space the triangles `pieces` of `plane` sweep along its normal, from the height
    /// `lo` to `hi`, hold just what the tool may gain without changing the result: nothing of
    /// the bodies (a cut), or one body's material (a join)? Decided on the bodies' faces (see
    /// [`kernel::Clearance`]), so a wall, a rib, a gap or a hole anywhere in it counts.
    /// `margin`: how far the region reaches past the triangles sideways (negative: their sides
    /// are drawn in, so that faces lying against them don't count).
    fn harmless(&self, plane: &Plane, pieces: &[[Vec2; 3]], margin: f64, lo: f64, hi: f64) -> bool {
        let n = plane.normal();
        let faces: Vec<([Vec3; 3], f64)> = pieces.iter().map(|t| (t.map(|p| plane.to_world(p)), margin)).collect();
        let mut space = solidcraft_geom::Aabb3::EMPTY;
        for p in faces.iter().flat_map(|(t, _)| t) {
            space.add(*p + n * lo);
            space.add(*p + n * hi);
        }
        let mut full = false;
        for (body, mesh, faces_of) in &self.bodies {
            // A body well away from that space holds none of it (its faces lie within the
            // display mesh's chord tolerance of the mesh).
            let (b, pad) = (mesh.bounds(), margin.abs() + 2.0 * body.display_tol());
            let away = |lo: Vec3, hi: Vec3| lo.x > hi.x + pad || lo.y > hi.y + pad || lo.z > hi.z + pad;
            if !b.is_empty() && (away(b.min, space.max) || away(space.min, b.max)) {
                continue;
            }
            let Some(clearance) = faces_of.get_or_init(|| kernel::Clearance::new(&body.body, body.body.size() * Self::CHORD)) else { return false };
            match clearance.swept(&faces, n, lo, hi) {
                kernel::Occupancy::Mixed => return false,
                kernel::Occupancy::Full => full = true,
                kernel::Occupancy::Empty => {}
            }
        }
        full == (self.op == Operation::Join)
    }
}

/// The outward normal (away from the profile) at parameter `t` of a segment of a
/// counterclockwise loop: the tangent turned clockwise.
fn outward(s: &solidcraft_geom::Seg2, t: f64) -> Option<Vec2> {
    use solidcraft_geom::Seg2;
    match *s {
        Seg2::Line { a, b } => (b - a).normalized().map(|d| Vec2::new(d.y, -d.x)),
        Seg2::Arc { start, sweep, .. } => Some(Vec2::from_angle(start + sweep * t) * sweep.signum()),
        _ => None,
    }
}

/// A counterclockwise loop of lines and arcs, each moved out by `by` where `grow` says so: a
/// line moves out and meets its neighbours again, an arc keeps its centre and changes its
/// radius. `None` when an edge would turn round (too short for the move), edges in one line
/// would part, a segment would leave a tangent neighbour behind, or an arc would move at a
/// sharp corner.
fn offset_segs(segs: &[solidcraft_geom::Seg2], grow: &[bool], by: f64) -> Option<Vec<solidcraft_geom::Seg2>> {
    use solidcraft_geom::Seg2;
    let n = segs.len();
    let tangent = |a: &Seg2, b: &Seg2| a.end_tangent().cross(b.start_tangent()).abs() < 1e-6 && a.end_tangent().dot(b.start_tangent()) > 0.0;
    let step = |g: bool| if g { by } else { 0.0 };
    // Where segment `i` starts once moved: the corner of it and the one before.
    let mut starts = Vec::with_capacity(n);
    for i in 0..n {
        let (prev, cur, g0, g1) = (segs.get((i + n - 1) % n)?, segs.get(i)?, *grow.get((i + n - 1) % n)?, *grow.get(i)?);
        let corner = cur.start();
        if !g0 && !g1 {
            starts.push(corner);
        } else if tangent(prev, cur) {
            // Moved together, a tangent joint moves along the common normal and stays tangent.
            if g0 != g1 {
                return None;
            }
            starts.push(corner + outward(cur, 0.0)? * by);
        } else {
            let (Seg2::Line { a: a0, b: b0 }, Seg2::Line { a: a1, b: b1 }) = (*prev, *cur) else { return None };
            let (d0, d1) = ((b0 - a0).normalized()?, (b1 - a1).normalized()?);
            let (p0, p1) = (a0 + Vec2::new(d0.y, -d0.x) * step(g0), a1 + Vec2::new(d1.y, -d1.x) * step(g1));
            let den = d0.cross(d1);
            if den.abs() >= 1e-12 {
                starts.push(p0 + d0 * ((p1 - p0).cross(d1) / den));
            } else if g0 == g1 {
                starts.push(p1);
            } else {
                return None;
            }
        }
    }
    let mut out = Vec::with_capacity(n);
    for (i, seg) in segs.iter().enumerate() {
        let (a, b, g) = (*starts.get(i)?, *starts.get((i + 1) % n)?, *grow.get(i)?);
        out.push(match *seg {
            Seg2::Line { a: a0, b: b0 } if (b - a).dot(b0 - a0) > 0.0 => Seg2::Line { a, b },
            Seg2::Arc { center, radius, start, sweep } if radius + step(g) * sweep.signum() > 0.0 => {
                Seg2::Arc { center, radius: radius + step(g) * sweep.signum(), start, sweep }
            }
            _ => return None,
        });
    }
    Some(out)
}

/// Grow profile edges that lie on a face of a body away from the profile, by a small amount,
/// so that the boolean meets no coincident face there. `probe` maps a sketch point to the
/// world points where the tool would be (along the extrude, around the revolve): an edge with
/// material on one side and room on the other at three points along it may grow. It grows
/// only where `gains` finds the whole strip it would add (triangles in the sketch) harmless,
/// asked a quarter wider than it grows, so that the moved side comes near nothing either:
/// three points miss the walls at the edge's ends, a neighbour across a gap, or a wall thinner
/// than the step. Elsewhere the edge stays where it was drawn. Loops of lines and arcs are
/// adjusted: a line moves out, an arc keeps its centre and changes its radius (a rounded corner
/// of a face cut whole, #1275); an arc's strip, and a line's where it meets one, is asked from
/// a little off the round face the arc lies on (see [`Surroundings`]'s `round`). Edges in one
/// line move together or not at all; arcs move only where both of their ends meet tangent
/// neighbours that move with them, otherwise the loop is left as it is.
fn grow_profile_for_coplanar(sur: &Surroundings, r: &Region2, probe: &dyn Fn(Vec2) -> Vec<Vec3>, gains: &dyn Fn(&[[Vec2; 3]]) -> bool) -> Region2 {
    use solidcraft_geom::Seg2;
    let delta = (sur.size * 0.005).max(1e-3);
    let lp = r.outer.ccw();
    let segs = &lp.segs;
    if segs.is_empty() || !segs.iter().all(|s| matches!(s, Seg2::Line { .. } | Seg2::Arc { .. })) {
        return r.clone();
    }
    let n = segs.len();
    // Many probes on one mesh each: indexed, the same answers as `Mesh::contains`.
    let inside: Vec<_> = sur.bodies.iter().map(|(_, m, _)| m.inside_index()).collect();
    let inside_any = |p: Vec3| inside.iter().any(|m| m.contains(p));
    let want_outer = sur.op == Operation::Join;
    let mut grow: Vec<bool> = segs
        .iter()
        .map(|seg| {
            if let Seg2::Arc { radius, sweep, .. } = *seg
                && sweep < 0.0
                && radius <= 2.0 * delta
            {
                // A concave arc too small to shrink by the step.
                return false;
            }
            // Is the edge on a body face? Material on exactly one side of it.
            let mids: Vec<(Vec2, Vec2)> = [0.25, 0.5, 0.75].iter().filter_map(|t| Some((seg.point_at(*t), outward(seg, *t)?))).collect();
            if mids.len() < 3 {
                return false;
            }
            let side = |k: f64| mids.iter().flat_map(|(m, out)| probe(*m + *out * (delta * k))).map(&inside_any).collect::<Vec<bool>>();
            let (outer, inner) = (side(0.5), side(-0.5));
            let on_face = outer.iter().all(|x| *x == want_outer) && inner.iter().any(|x| *x != want_outer);
            on_face && side(1.0).iter().all(|x| *x == want_outer)
        })
        .collect();
    let in_line = |i: usize| match (segs.get(i % n), segs.get((i + 1) % n)) {
        (Some(Seg2::Line { a: a0, b: b0 }), Some(Seg2::Line { a: a1, b: b1 })) => {
            matches!(((*b0 - *a0).normalized(), (*b1 - *a1).normalized()), (Some(d0), Some(d1)) if d0.cross(d1).abs() < 1e-12)
        }
        _ => false,
    };
    let round = |k: usize| matches!(segs.get(k % n), Some(Seg2::Arc { .. }));
    // What segment `i` gains moving out to `moved`, as triangles in the sketch; `None` when
    // nothing of it can be asked.
    let strip = |i: usize, seg: &Seg2, moved: &Seg2| -> Option<Vec<[Vec2; 3]>> {
        match (*seg, *moved) {
            (Seg2::Line { a, b }, Seg2::Line { a: a2, b: b2 }) => {
                // Its ends drawn back from the round faces of arcs it runs into.
                let d = (b - a).normalized()?;
                let s0 = if round(i + n - 1) { sur.round } else { 0.0 };
                let s1 = if round(i + 1) { sur.round } else { 0.0 };
                if (b - a).len() <= s0 + s1 {
                    return None;
                }
                let (a, a2, b, b2) = (a + d * s0, a2 + d * s0, b - d * s1, b2 - d * s1);
                Some(vec![[a, b, b2], [a, b2, a2]])
            }
            (Seg2::Arc { center, radius, start, sweep }, Seg2::Arc { radius: far, .. }) => {
                // From `round` off the face out to the moved arc, in pieces whose chords stand off
                // the circles by less than `round`: on circles drawn round those circles' chords
                // where the arc is convex (the chords then stay off the face and reach past the
                // moved arc), on the circles themselves where it is concave.
                let near = radius + sur.round * sweep.signum();
                let steps = ((sweep.abs() / (8.0 * sur.round / radius.max(far)).sqrt()).ceil() as usize).clamp(1, 256);
                let scale = if sweep > 0.0 { 1.0 / (sweep.abs() / steps as f64 / 2.0).cos() } else { 1.0 };
                let at = |k: usize, rad: f64| center + Vec2::from_angle(start + sweep * k as f64 / steps as f64) * (rad * scale);
                Some((0..steps).flat_map(|k| [[at(k, near), at(k + 1, near), at(k + 1, far)], [at(k, near), at(k + 1, far), at(k, far)]]).collect())
            }
            _ => None,
        }
    };
    for _ in 0..=n {
        // Edges in one line move together or not at all.
        while let Some(i) = (0..n).find(|&i| grow.get(i) != grow.get((i + 1) % n) && in_line(i)) {
            grow.iter_mut().enumerate().filter(|(k, _)| *k == i || *k == (i + 1) % n).for_each(|(_, g)| *g = false);
        }
        if !grow.contains(&true) {
            return r.clone();
        }
        let Some(reach) = offset_segs(segs, &grow, delta * 1.25) else { return r.clone() };
        let mut dropped = false;
        for (i, (seg, moved)) in segs.iter().zip(&reach).enumerate() {
            if grow.get(i) == Some(&true)
                && !strip(i, seg, moved).is_some_and(|pieces| gains(&pieces))
                && let Some(g) = grow.get_mut(i)
            {
                *g = false;
                dropped = true;
            }
        }
        if !dropped {
            return match offset_segs(segs, &grow, delta) {
                Some(segs) => Region2 { outer: solidcraft_geom::Loop2 { segs }, holes: r.holes.clone() },
                None => r.clone(),
            };
        }
    }
    r.clone()
}

/// Booleans struggle with coincident faces (a cut starting on the face it cuts, a boss
/// starting on the face it joins). Extending the tool past such an end changes nothing when the
/// space beyond is empty (cut) or already material (join), so do exactly that. Only an end
/// that lies on a body face parallel to it moves, by less than half the way to the next such
/// face level, and only when the whole slab it gains (asked a quarter further than it moves)
/// is harmless on the bodies' faces (see [`Surroundings::harmless`]): sample points miss a rib
/// beyond the end, a thin floor between a recess above and a ring pocket below, or a hole or a
/// gap behind it.
fn extend_for_coplanar(sur: &Surroundings, plane: &Plane, r: &Region2, lo: f64, hi: f64) -> (f64, f64) {
    let size = sur.size;
    let delta = (size * 0.01).max(1e-3).min((hi - lo).abs().max(1e-3));
    let samples = region_samples(r);
    let n = plane.normal();
    // Many probes on one mesh each: indexed, the same answers as `Mesh::contains`.
    let inside: Vec<_> = sur.bodies.iter().map(|(_, m, _)| m.inside_index()).collect();
    let inside_any = |p: Vec3| inside.iter().any(|m| m.contains(p));
    // Heights of the bodies' faces parallel to the profile plane.
    let tol = (size * 1e-7).max(1e-9);
    let levels: Vec<f64> = sur
        .bodies
        .iter()
        .map(|(_, m, _)| m)
        .flat_map(|m| {
            m.triangles.iter().filter_map(|t| {
                let [a, b, c] = m.tri(t)?;
                let normal = (b - a).cross(c - a).normalized()?;
                (normal.dot(n).abs() > 1.0 - 1e-9).then(|| (a - plane.origin).dot(n))
            })
        })
        .collect();
    // How far the end at `h` may move in `dir`: zero unless a face lies on it, then short of
    // half the gap to the next face level beyond it.
    let room = |h: f64, dir: f64| {
        if !levels.iter().any(|z| (z - h).abs() < tol) {
            return 0.0;
        }
        let gap = levels.iter().map(|z| (z - h) * dir).filter(|g| *g > tol).fold(f64::INFINITY, f64::min);
        delta.min(gap * 0.5)
    };
    // The profile's triangles; curved sides stand off their chords by up to `chord`.
    let chord = (size * 2e-4).max(1e-6);
    let straight = std::iter::once(&r.outer).chain(&r.holes).flat_map(|l| &l.segs).all(|s| matches!(s, solidcraft_geom::Seg2::Line { .. }));
    let ok_beyond = |h: f64, dir: f64, step: f64| {
        let sampled = samples.iter().all(|s| {
            let base = plane.to_world(*s);
            [0.5, 1.0].iter().all(|k| inside_any(base + n * (h + dir * step * k)) == (sur.op == Operation::Join))
        });
        if !sampled {
            return false;
        }
        let pieces = r.triangulate(chord);
        // Triangles that leave part of a straight profile uncovered leave it unasked.
        let covered: f64 = pieces.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).abs() * 0.5).sum();
        if pieces.is_empty() || (straight && (covered - r.area()).abs() > 1e-7 * r.area()) {
            return false;
        }
        let (h0, h1) = if dir > 0.0 { (h + sur.skin, h + step * 1.25) } else { (h - step * 1.25, h - sur.skin) };
        sur.harmless(plane, &pieces, if straight { -sur.skin } else { 2.0 * chord }, h0, h1)
    };
    let (down, up) = (room(lo, -1.0), room(hi, 1.0));
    let lo2 = if down > tol && ok_beyond(lo, -1.0, down) { lo - down } else { lo };
    let hi2 = if up > tol && ok_beyond(hi, 1.0, up) { hi + up } else { hi };
    (lo2, hi2)
}

/// Remove ends of a join tool already wholly inside its targets. A tab extending
/// a rib otherwise shares the rib's side faces through its buried portion; on a
/// chamfered base those coincident faces can defeat every boolean retry (#2059).
/// Only straight profiles are trimmed, after checking the entire swept material
/// against the targets' faces, with the same clearance used when extending ends.
fn trim_embedded_join_ends(sur: &Surroundings, plane: &Plane, r: &Region2, lo: f64, hi: f64) -> (f64, f64) {
    if sur.op != Operation::Join
        || !std::iter::once(&r.outer).chain(&r.holes).flat_map(|l| &l.segs).all(|s| matches!(s, solidcraft_geom::Seg2::Line { .. }))
    {
        return (lo, hi);
    }
    let pieces = r.triangulate(1e-6);
    let covered: f64 = pieces.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).abs() * 0.5).sum();
    if pieces.is_empty() || (covered - r.area()).abs() > 1e-7 * r.area() {
        return (lo, hi);
    }
    let n = plane.normal();
    let mut levels: Vec<f64> = sur
        .bodies
        .iter()
        .flat_map(|(_, mesh, _)| {
            mesh.triangles.iter().filter_map(|t| {
                let [a, b, c] = mesh.tri(t)?;
                let normal = (b - a).cross(c - a).normalized()?;
                (normal.dot(n).abs() > 1.0 - 1e-9).then(|| (a - plane.origin).dot(n))
            })
        })
        .filter(|h| *h > lo + sur.skin && *h < hi - sur.skin)
        .collect();
    levels.sort_by(f64::total_cmp);
    levels.dedup_by(|a, b| (*a - *b).abs() < sur.skin);
    if levels.len() > 256 {
        return (lo, hi);
    }
    let start = levels.iter().rev().copied().find(|h| sur.harmless(plane, &pieces, -sur.skin, lo + sur.skin, h - sur.skin)).unwrap_or(lo);
    let end =
        levels.iter().copied().find(|h| *h > start + sur.skin && sur.harmless(plane, &pieces, -sur.skin, h + sur.skin, hi - sur.skin)).unwrap_or(hi);
    (start, end)
}

/// Whether a tapered tool, run on by `depth` behind the plane it starts in (`dir` the way it
/// extrudes), stays where running on changes nothing: in the targets for a join, out of them for
/// a cut. Its sides keep their slope, so the section there is the profile grown by up to `widen`
/// all round; that grown outline is probed at two depths.
#[allow(clippy::too_many_arguments)]
fn taper_run_on_fits(st: &ModelState, start: &Plane, dir: Vec3, r: &Region2, depth: f64, widen: f64, op: Operation, targets: &[String]) -> bool {
    const MAX_SEGMENTS: usize = 2_000;
    let meshes: Vec<Arc<Mesh>> = st.bodies.iter().filter(|b| targets.is_empty() || targets.contains(&b.name)).map(|b| b.mesh()).collect();
    if meshes.is_empty() || !depth.is_finite() || !widen.is_finite() {
        return false;
    }
    let want = op == Operation::Join;
    // Each loop counterclockwise with the side away from the material: right of the outer loop,
    // left of a hole.
    let loops: Vec<(solidcraft_geom::Loop2, f64)> = std::iter::once((r.outer.ccw(), 1.0)).chain(r.holes.iter().map(|h| (h.ccw(), -1.0))).collect();
    let count: usize = loops.iter().map(|(l, _)| l.segs.len()).sum();
    if count > MAX_SEGMENTS {
        return false;
    }
    loops.iter().all(|(lp, side)| {
        lp.segs.iter().all(|seg| {
            [0.0, 0.25, 0.5, 0.75].iter().all(|t| {
                let Some(d) = seg.derivative(*t).normalized() else { return true };
                let out = Vec2::new(d.y, -d.x) * *side;
                [0.5, 1.0].iter().all(|k| {
                    let p = start.to_world(seg.point_at(*t) + out * (widen * k)) - dir * (depth * k);
                    meshes.iter().any(|m| m.contains(p)) == want
                })
            })
        })
    })
}

/// How high (mm) a tapered tool's sides stand straight where it starts on a face it can't run
/// on into.
const TAPER_FOOT: f64 = 0.01;

/// Whether a face of the targets (all bodies when none are named) lies in the plane at height
/// `h` along `plane`'s normal.
fn face_level_at(st: &ModelState, plane: &Plane, h: f64, targets: &[String]) -> bool {
    let n = plane.normal();
    st.bodies.iter().filter(|b| targets.is_empty() || targets.contains(&b.name)).any(|b| {
        let m = b.mesh();
        let tol = (b.body.size() * 1e-7).max(1e-9);
        m.triangles.iter().any(|t| {
            m.tri(t).is_some_and(|[a, b, c]| {
                (b - a).cross(c - a).normalized().is_some_and(|normal| normal.dot(n).abs() > 1.0 - 1e-9)
                    && ((a - plane.origin).dot(n) - h).abs() < tol
            })
        })
    })
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
        .map(|(i, b)| {
            let bb = b.mesh().bounds();
            let q = Vec3::new(p.x.clamp(bb.min.x, bb.max.x), p.y.clamp(bb.min.y, bb.max.y), p.z.clamp(bb.min.z, bb.max.z));
            (i, q.dist(*p))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
        .ok_or_else(|| DocError::Invalid("there is no body".into()))
}

/// A hole stays on its face when the model changes: a position now inside a body moves back
/// against the drilling direction to the surface, one now above it moves down onto it. Only the
/// bodies in `scope` count (every body when it is empty, see [`hole_scope`]).
fn hole_on_surface(st: &ModelState, p: Vec3, dir: Vec3, scope: &[String]) -> Vec3 {
    let size = st.bodies.iter().map(|b| b.body.size()).fold(1.0, f64::max);
    let eps = size * 1e-6;
    let mut best: Option<Vec3> = None;
    for b in st.bodies.iter().filter(|b| in_hole_scope(scope, b)) {
        let m = b.mesh();
        // Already on the surface: the drill enters right here.
        if let Some((t, _)) = m.raycast(p - dir * eps, dir)
            && t < eps * 4.0
        {
            return p;
        }
        let q = if m.contains(p) { m.raycast(p, -dir).map(|(t, _)| p - dir * t) } else { m.raycast(p, dir).map(|(t, _)| p + dir * t) };
        if let Some(q) = q
            && best.is_none_or(|bq| q.dist(p) < bq.dist(p))
        {
            best = Some(q);
        }
    }
    best.unwrap_or(p)
}

/// The faces of the model near `p`: for each, how far `p` is from it, the outward normal there
/// (the surface's own normal for planes, cylinders and spheres, turned the way the face looks out
/// of its body; the nearest mesh facet's otherwise) and, for a plane, how far `p` is from the
/// plane itself. Only faces within `reach` of `p`, nearest first.
fn faces_through(st: &ModelState, p: Vec3, reach: f64) -> Vec<(f64, Vec3, Option<f64>)> {
    let mut out: Vec<(f64, Vec3, Option<f64>)> = Vec::new();
    for b in st.bodies.iter().filter(|b| !b.body.is_mesh()) {
        let m = b.mesh();
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
        for (f, (d, facet)) in near {
            let (exact, off) = match kernel::face_surface(&b.body, f as usize) {
                Some(kernel::FaceSurface::Plane { normal, point }) => (Some(*normal), Some((p - point).dot(normal).abs())),
                Some(kernel::FaceSurface::Cylinder { origin, axis, .. }) => {
                    let v = p - origin;
                    ((v - axis * v.dot(axis)).normalized(), None)
                }
                Some(kernel::FaceSurface::Sphere { center, .. }) => ((p - center).normalized(), None),
                _ => (None, None),
            };
            let n = match exact {
                Some(n) if n.dot(facet) < 0.0 => -n,
                Some(n) => n,
                None => facet,
            };
            out.push((d, n, off));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out
}

/// The way a hole placed at `p` drills: into the material, against the outward normal of the
/// face `p` lies on. A point on no face (a planar face's centroid outside it, say) takes the
/// nearest planar face whose plane it lies in, else the face nearest it within its body's pick
/// tolerance. None when no face is near.
pub fn hole_direction_at(st: &ModelState, p: Vec3) -> Option<Vec3> {
    let size = st.bodies.iter().map(|b| b.body.size()).fold(1.0, f64::max);
    let faces = faces_through(st, p, f64::INFINITY);
    if let Some((d, n, _)) = faces.first()
        && *d <= size * 1e-4
    {
        return Some(-*n);
    }
    if let Some((_, n, _)) = faces.iter().find(|(_, _, off)| off.is_some_and(|o| o <= size * 1e-4)) {
        return Some(-*n);
    }
    let reach = st.bodies.iter().map(crate::appearance::pick_tolerance).fold(0.0, f64::max);
    faces.first().filter(|(d, _, _)| *d <= reach).map(|(_, n, _)| -*n)
}

/// A hole placed on a face drills into the material: when its direction leaves every face it
/// starts on (as designs saved before format 18 can say: their direction was taken from any face
/// whose plane passed near the point, such as the other side of a thin plate or a face of another
/// body at the same height), it is mirrored in the face it starts on, so it goes in at the same
/// angle. A direction that enters a face there, or a spot on no face, is kept.
fn hole_dir_into_face(st: &ModelState, p: Vec3, dir: Vec3) -> Vec3 {
    let size = st.bodies.iter().map(|b| b.body.size()).fold(1.0, f64::max);
    let faces = faces_through(st, p, size * 1e-5);
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
/// [`picked_bodies`]).
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
        for (i, n) in idx.into_iter().zip(crate::naming::edge_names_at(b, &at)) {
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
    let now = b.tessellate((size * 1e-2).max(1e-2)).map(|m| m.bounds()).ok();
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
fn blend_target(state: &ModelState, body: &Option<crate::BodyName>, edges: &[Vec3]) -> Result<usize> {
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
/// runs through the pick `p`, else where the name is one edge (not split), else the nearest.
fn named_body(st: &ModelState, name: &str, p: Vec3) -> Option<usize> {
    // Names start with the feature that made the face: only bodies with such a face can hold it.
    let bare = crate::naming::legacy_name(name).unwrap_or(name);
    let token = bare.find(':').map(|k| &bare[..=k]);
    let mut best: Option<(usize, (bool, bool, f64))> = None;
    for (i, b) in st.bodies.iter().enumerate() {
        if b.body.is_mesh() || token.is_some_and(|t| !crate::naming::face_names(b).iter().any(|n| n.contains(t))) {
            continue;
        }
        let Ok(hit) = crate::naming::locate_edge(b, name, p) else { continue };
        let score = (hit.on_point, !hit.split, -hit.point.dist(p));
        if best.as_ref().is_none_or(|(_, s)| score.partial_cmp(s).is_some_and(|o| o.is_gt())) {
            best = Some((i, score));
        }
    }
    best.map(|(i, _)| i)
}

/// The body (index) an edge picked at `p` lies on: the body with an edge through it.
fn body_on_edge(st: &ModelState, p: Vec3) -> Option<usize> {
    let mut best: Option<(usize, f64)> = None;
    for (i, b) in st.bodies.iter().enumerate().filter(|(_, b)| !b.body.is_mesh()) {
        let size = b.body.size();
        let tight = (size * 2e-3).max(1e-3);
        if let Ok(Some((_, d))) = b.body.nearest_edge(p, (size * 1e-3).max(1e-3))
            && d <= tight
            && best.is_none_or(|(_, bd)| d < bd)
        {
            best = Some((i, d));
        }
    }
    best.map(|(i, _)| i)
}

/// The body (index) each edge of a blend is on: the named body; else, for each edge, the body
/// its stored name finds it on. A named edge that is gone stays on the bodies the feature's other
/// edges were found on; an unnamed one goes to the body the pick lies on an edge of; failing
/// both, the body nearest the first pick (as before blends were split by body). A named edge is
/// never moved onto another body because that body's edge is nearer.
fn blend_bodies(st: &ModelState, f: &Feature, body: &Option<crate::BodyName>, pts: &[Vec3]) -> Result<Vec<usize>> {
    // Named, or only one body to blend: no lookup.
    if body.is_some() || st.bodies.iter().filter(|b| !b.body.is_mesh()).count() < 2 {
        let ti = blend_target(st, body, pts)?;
        return Ok(vec![ti; pts.len()]);
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
    for ((b, n), p) in by_name.into_iter().zip(&names).zip(pts) {
        let found = match (b, n) {
            (Some(ti), _) => Some(ti),
            // A named edge that is gone stays with the feature's other bodies: another body's
            // edge where it was picked is not it.
            (None, Some(_)) => nearest_named(*p).or_else(|| body_on_edge(st, *p)),
            (None, None) => body_on_edge(st, *p).or_else(|| nearest_named(*p)),
        };
        let ti = match (found, fallback) {
            (Some(ti), _) => ti,
            (None, Some(ti)) => ti,
            (None, None) => *fallback.insert(blend_target(st, &None, pts)?),
        };
        out.push(ti);
    }
    Ok(out)
}

/// The edges `idx` of blend `f` found on body `mb`: by name first; a name that is gone falls back
/// to the point, with a warning.
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
        let stored = f.edge_names.get(i).filter(|n| !n.is_empty());
        let trusted = trusted_edge_name(f, i, p);
        // A name recorded on another body (see `trusted_edge_name`): the pick alone.
        if stored.is_some() && trusted.is_none() {
            let mut w = None;
            match resolve_edges(&mb.body, &[p], &[], &mut w) {
                Ok(q) => out.extend(q),
                Err(_) => notes.push("an edge reference was not found at its point and was skipped".to_string()),
            }
            notes.extend(w);
            continue;
        }
        match trusted.map(|n| (n, crate::naming::locate_edge(mb, n, p))) {
            Some((n, Ok(hit))) => {
                if let Some(note) = hit.note(n) {
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
            Some((_, Err(_))) | None => {
                let mut w = None;
                let refs: Vec<EdgeRef> = f.edge_refs.get(i).cloned().into_iter().collect();
                let skipped = format!("edge `{}` no longer exists and was skipped", stored.map(String::as_str).unwrap_or("?"));
                // The name is gone. A pick on the axis of a round must not fall through to the
                // tangency seam (#775). A sharp edge that moved still resolves from the point,
                // which an edit relies on.
                if pick_lies_on_a_round_axis(&mb.body, p) {
                    notes.push(skipped);
                } else if let Ok(q) = resolve_edges(&mb.body, &[p], &refs, &mut w) {
                    out.extend(q);
                    notes.push(match stored {
                        Some(n) => format!("edge `{n}` no longer exists; using the edge at the picked point"),
                        None => "an edge reference without a name was found by its point".into(),
                    });
                } else {
                    notes.push(skipped);
                }
            }
        }
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

/// Body `mb` with blend `f` applied at `edges` (radius or distance `r`), the radii a fitted fillet
/// used (when it reduced any) and the edges it tapered.
fn blend_body(
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    mb: &ModelBody,
    edges: &[Vec3],
    r: f64,
    warning: &mut Option<String>,
) -> Result<(Traced, Option<Vec<f64>>, Tapered)> {
    let input = mb.traced();
    let (mut used, mut tapered) = (None, Vec::new());
    // The blended body, and the lineage of what the last blend started from.
    let (nb, from) = match &f.kind {
        FeatureKind::Fillet { corner: crate::FilletCorner::G2, .. } => (kernel::fillet_g2_corners(&mb.body, edges, r)?, input),
        FeatureKind::Fillet { style: crate::FilletStyle::Curvature, .. } => (kernel::fillet_g2(&mb.body, edges, r)?, input),
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
        FeatureKind::Fillet { fit, .. } => {
            let (b, u) = fillet_fitted(&mb.body, edges, r, *fit)?;
            if let Some(u) = &u {
                append_warning(warning, reduced_note(u, r));
            }
            used = u;
            (b, input)
        }
        FeatureKind::Chamfer { distance2, angle, flip, .. } => (chamfer_with(vals, &mb.body, edges, r, distance2, angle, *flip)?, input),
        _ => return Err(DocError::Invalid("blend".into())),
    };
    // Keeping the input's planar certificates may join faces; their history stays over the same
    // operand.
    Ok((Traced::step(nb.with_preserved_planar_sources(&mb.body)?, &[&from]), used, tapered))
}

/// The revolve axis in the sketch's plane coordinates (a point and a direction). `st` is the model
/// an axis on an edge or face of it is found in; `construct` gives a construction axis, by name, in
/// the sketch's 3D frame.
pub(crate) fn revolve_axis(
    ss: &SolvedSketch,
    axis: &AxisRef,
    st: &ModelState,
    construct: &dyn Fn(&str) -> Result<(Vec3, Vec3)>,
) -> Result<(Vec2, Vec2)> {
    match axis {
        AxisRef::SketchLine { curve } => {
            let ci = ss.sketch.curve_index(curve).ok_or_else(|| DocError::Unknown(format!("sketch line `{curve}`")))?;
            match ss.sketch.curves.get(ci).map(|c| &c.kind) {
                Some(CurveKind::Line { a, b }) => {
                    let (pa, pb) = (ss.sketch.point(*a).unwrap_or_default(), ss.sketch.point(*b).unwrap_or_default());
                    Ok((pa, pb - pa))
                }
                _ => Err(DocError::Invalid(format!("`{curve}` is not a line"))),
            }
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
        AxisRef::Line { origin, dir } => line_in_sketch_plane(ss, origin.0, dir.0),
        AxisRef::Model { geo } => {
            let (o, d) = crate::construct::model_axis(st, geo)?;
            line_in_sketch_plane(ss, o, d)
        }
        AxisRef::Construction { name } => {
            let (o, d) = construct(name)?;
            line_in_sketch_plane(ss, o, d)
        }
    }
}

/// The model a revolve axis is found in: what construction geometry of component `comp` is built
/// on when the axis is an edge or face of it (only then is it worth placing).
pub(super) fn axis_state<'a>(doc: &Document, st: &'a ModelState, comp: u64, axis: &AxisRef) -> std::borrow::Cow<'a, ModelState> {
    match axis {
        AxisRef::Model { .. } => construction_state(doc, st, comp),
        _ => std::borrow::Cow::Borrowed(st),
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
    let dir = sketch_hole_dir(st, &spots, ss.plane.normal(), &[]);
    Some(if flip { -dir } else { dir })
}

fn sketch_hole_dir(st: &ModelState, spots: &[Vec3], n: Vec3, scope: &[String]) -> Vec3 {
    let down = -n;
    let any = |d: Vec3, ahead: bool| spots.iter().any(|p| hole_reaches(st, *p, d, ahead, scope));
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

fn in_hole_scope(scope: &[String], b: &ModelBody) -> bool {
    scope.is_empty() || scope.contains(&b.name)
}

/// Where a hole feature drills (each on the surface) and in which direction: at its sketch
/// points perpendicular to the sketch (into the material), else at its position.
fn hole_spots(vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature) -> Result<(Vec<Vec3>, Vec3)> {
    let FeatureKind::Hole { position, at, direction, points, .. } = &f.kind else { return Err(DocError::Invalid("not a hole".into())) };
    let (spots, dir) = match points {
        Some(sp) if !sp.single => {
            let ss =
                st.sketch(sp.sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {} (it must come earlier in the timeline)", sp.sketch)))?;
            let mut v = Vec::new();
            for id in &sp.ids {
                let p = ss.sketch.points.iter().find(|p| &p.id == id).ok_or_else(|| DocError::Unknown(format!("sketch point `{id}`")))?;
                v.push(ss.plane.to_world(p.pos));
            }
            let dir = sketch_hole_dir(st, &v, ss.plane.normal(), hole_scope(f));
            let dir = if sp.flip { -dir } else { dir };
            (v, dir)
        }
        _ => {
            let dir = direction.normalized().ok_or_else(|| DocError::Invalid("hole direction".into()))?;
            let at = hole_on_surface(st, single_hole_position(vals, st, position, at, points)?, dir, &[]);
            let at = with_references(vals, st, f, at, dir)?;
            let dir = hole_dir_into_face(st, at, dir);
            return Ok((vec![hole_on_surface(st, at, dir, &[])], dir));
        }
    };
    if spots.is_empty() || spots.len() > 10_000 {
        return Err(DocError::Invalid("a hole needs 1…10000 points".into()));
    }
    Ok((spots.into_iter().map(|p| hole_on_surface(st, p, dir, hole_scope(f))).collect(), dir))
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
fn hole_spots_raw(vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature) -> Result<(Vec<Vec3>, Vec3)> {
    let FeatureKind::Hole { position, at, direction, points, .. } = &f.kind else { return Err(DocError::Invalid("not a hole".into())) };
    match points {
        Some(sp) if !sp.single => {
            let ss = st.sketch(sp.sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {}", sp.sketch)))?;
            let v: Vec<Vec3> =
                sp.ids.iter().filter_map(|id| ss.sketch.points.iter().find(|p| &p.id == id)).map(|p| ss.plane.to_world(p.pos)).take(10_000).collect();
            let dir = sketch_hole_dir(st, &v, ss.plane.normal(), hole_scope(f));
            let dir = if sp.flip { -dir } else { dir };
            Ok((v, dir))
        }
        _ => {
            let dir = direction.normalized().ok_or_else(|| DocError::Invalid("hole direction".into()))?;
            let p = single_hole_position(vals, st, position, at, points)?;
            let p = if hole_references(f).is_empty() { p } else { with_references(vals, st, f, hole_on_surface(st, p, dir, &[]), dir)? };
            Ok((vec![p], hole_dir_into_face(st, hole_on_surface(st, p, dir, &[]), dir)))
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
fn cylinder_base(vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature, base: &crate::PointExpr, axis: Vec3) -> Result<Vec3> {
    let at = point(vals, base)?;
    with_references(vals, st, f, at, -axis)
}

/// Where a Single hole (or a cylinder) with reference edges (#37) is placed: at its distances
/// from the edges' lines, in the plane of the face at `at` (on the surface), the edges found by
/// their names, else their points. A feature without reference edges stays at `at`.
fn with_references(vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature, at: Vec3, dir: Vec3) -> Result<Vec3> {
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
    if !crate::hole_reference::on_reference_face(st, &frame, centre) {
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
fn parallel_tapped_wall(st: &ModelState, entry: Vec3, inward: Vec3, radius: f64) -> Option<Vec3> {
    let tol = (radius * 1e-6).max(1e-6);
    let mut best: Option<(f64, Vec3)> = None;
    for body in &st.bodies {
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
            if hi > lo + 1e-9 && best.as_ref().is_none_or(|(old, _)| lo < *old) {
                best = Some((lo, entry + inward * ((lo + hi) * 0.5) + inward.any_perp() * radius));
            }
        }
    }
    best.map(|(_, wall)| wall)
}

/// The cutting tools of a hole feature at every spot, each with its exact geometry when it is a
/// straight cylinder (see [`hole_tools_and_prisms`]).
fn hole_feature_tools(vals: &BTreeMap<String, Value>, st: &ModelState, f: &Feature) -> Result<Vec<(Traced, Option<more::PrimitivePrism>)>> {
    let FeatureKind::Hole { diameter, depth, hole, to, .. } = &f.kind else { return Err(DocError::Invalid("not a hole".into())) };
    let (spots, dir) = hole_spots(vals, st, f)?;
    if spots.is_empty() || spots.len() > 10_000 {
        return Err(DocError::Invalid("a hole needs 1…10000 points".into()));
    }
    let mut tools = Vec::new();
    for at in spots {
        let depth = hole_depth_to(st, &at, dir, &to.map(Vec3::from), depth)?;
        tools.extend(hole_tools_and_prisms(vals, st, &at, dir, diameter, &depth, hole)?);
    }
    Ok(tools)
}

/// A hole drilled square into sheet metal whose tools are straight cylinders (a plain hole, a
/// counterbore) is machined like the equivalent extrude cut, so the sheet keeps it as a cut-out
/// or pocket of its flat pattern (#560). Returns the sheet bodies it machined; none, with the
/// model as it was, when a tool that meets a sheet can't be kept that way (the ordinary cut then
/// refuses it). Pieces the hole parts from a sheet's base are named in `warning`.
fn sheet_hole_cut(
    st: &mut ModelState,
    f: &Feature,
    tools: &[(Traced, Option<more::PrimitivePrism>)],
    warning: &mut Option<String>,
) -> Result<Vec<String>> {
    if st.sheets.is_empty() {
        return Ok(Vec::new());
    }
    let saved = (st.sheets.clone(), st.bodies.clone());
    let mut handled: Vec<String> = Vec::new();
    // Noted only when every tool was machined here.
    let mut parted = None;
    // Many plain drills along one axis (a hole on each of a pattern's points): all in one pass.
    let prisms: Option<Vec<more::PrimitivePrism>> = tools.iter().map(|(_, prism)| *prism).collect();
    if let Some(prisms) = prisms {
        match more::sheet_drill_cuts(f, st, &prisms, &mut parted) {
            Ok(Some(done)) => {
                if let Some(note) = parted {
                    append_warning(warning, note);
                }
                return Ok(done);
            }
            // Not all of them that way, or refused at once: one by one, from the model as it was.
            Ok(None) | Err(_) => {
                (st.sheets, st.bodies) = saved.clone();
                parted = None;
            }
        }
    }
    for (tool, prism) in tools {
        let tool = &tool.body;
        let done = match prism {
            Some(prism) => match more::sheet_primitive_cut(f, st, tool, *prism, Operation::Cut, true, &mut parted) {
                Ok(done) => done,
                // Drilled along a panel (into a rim) the hole is no flat cut-out: a bore of
                // that panel, when it stays inside it.
                Err(e) => match more::sheet_bore_cut(f, st, *prism)? {
                    Some(done) => done,
                    None => return Err(e),
                },
            },
            None => Vec::new(),
        };
        let done = match (done.is_empty(), prism) {
            (true, Some(prism)) => more::sheet_bore_cut(f, st, *prism)?.unwrap_or_default(),
            _ => done,
        };
        if done.is_empty() {
            let mut meets = false;
            for sheet in &st.sheets {
                if let Some(mb) = st.body(&sheet.body)
                    && kernel::boolean(&mb.body, tool, BoolOp::Intersect)?.is_some()
                {
                    meets = true;
                    break;
                }
            }
            if meets {
                (st.sheets, st.bodies) = saved;
                return Ok(Vec::new());
            }
        }
        for name in done {
            if !handled.contains(&name) {
                handled.push(name);
            }
        }
    }
    if let Some(note) = parted {
        append_warning(warning, note);
    }
    Ok(handled)
}

/// The cutting tools of one hole at `position`, drilling along `dir`.
#[allow(clippy::too_many_arguments)]
fn hole_tools(
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    position: &Vec3,
    dir: Vec3,
    diameter: &str,
    depth: &Option<crate::Expr>,
    hole: &HoleKind,
) -> Result<Vec<Traced>> {
    Ok(hole_tools_and_prisms(vals, st, position, dir, diameter, depth, hole)?.into_iter().map(|(tool, _)| tool).collect())
}

/// The scale a hole's tools are sized by: the largest body the hole's axis meets (#2904), so a
/// large body elsewhere in the model changes neither the tool's reach above the face (and so
/// the countersink's outline) nor its depth. A hole whose axis meets no body is sized by the
/// largest body of the model, as before.
fn hole_axis_size(st: &ModelState, position: &Vec3, dir: Vec3) -> f64 {
    let along = |b: &ModelBody| {
        // The axis (an unbounded line) against the body's box, grown by the display tolerance.
        let bb = b.mesh().bounds();
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

/// [`hole_tools`], each with its exact geometry when it is a straight cylinder along the axis
/// (a plain hole, a counterbore, a drill point left in the air past a through hole), so sheet
/// metal can keep it as a flat cut-out or pocket. Each tool's faces are named by the part of the
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
) -> Result<Vec<(Traced, Option<more::PrimitivePrism>)>> {
    let r = val(vals, diameter, Kind::Length)? / 2.0;
    if !(r > 1e-6) {
        return Err(DocError::Invalid("hole diameter must be positive".into()));
    }
    let size = hole_axis_size(st, position, dir);
    let top = (size * 0.01).max(0.1);
    let bottom = match depth {
        Some(d) => -val(vals, d, Kind::Length)?,
        None => -(size * 2.0 + 1.0),
    };
    if bottom >= 0.0 {
        return Err(DocError::Invalid("hole depth must be positive".into()));
    }
    // Tools are cylinders and cones made by (tapered) extrudes along the axis, cut one
    // after another, widest first.
    let plane = Plane::from_normal(*position, -dir).ok_or_else(|| DocError::Invalid("hole axis".into()))?;
    // Each tool's circle seam at its own angle, so seams don't line up between tools.
    let seam = std::cell::Cell::new(0.0);
    let disc = |rad: f64| {
        seam.set(seam.get() + 0.613);
        Region2 { outer: solidcraft_geom::Loop2::circle_from(Vec2::ZERO, rad, seam.get()), holes: vec![] }
    };
    let cyl = |rad: f64, lo: f64, hi: f64, part: &str| -> Result<(Traced, Option<more::PrimitivePrism>)> {
        let tool = kernel::extrude(&plane, &[disc(rad)], lo, hi)?.pop().ok_or_else(|| DocError::Invalid("hole tool".into()))?;
        let axis = plane.normal();
        let prism = more::PrimitivePrism::Cylinder { base: *position + axis * lo, axis, radius: rad, height: hi - lo };
        Ok((crate::lineage::part_tool(tool, part), Some(prism)))
    };
    // Reuse the existing drill-point tool for the narrow bore, independently of its entry.
    let bore = |tip_angle: Option<&crate::Expr>| -> Result<Vec<(Traced, Option<more::PrimitivePrism>)>> {
        if let Some(tip_angle) = tip_angle {
            let half = val(vals, tip_angle, Kind::Angle)? / 2.0;
            // A hole exactly as deep as the material: the point is in the air past the exit
            // face, which would lie on the drill point's shoulder (a through hole, as drilled).
            let at = |h: f64| *position + dir * (-h);
            let eps = size * 1e-6;
            let solid_at = |h: f64| st.bodies.iter().any(|b| b.body.contains(at(h)));
            let exits_at_shoulder = solid_at(bottom + eps) && !solid_at(bottom - eps);
            if depth.is_some() && half > 1e-3 && half < std::f64::consts::FRAC_PI_2 && !exits_at_shoulder {
                // One revolved tool (cylinder with a drill point).
                let tip = r / half.tan();
                let up = -dir;
                let side = up.any_perp();
                let rp = Plane::new(*position, side, up).ok_or_else(|| DocError::Invalid("hole axis".into()))?;
                let pts = [Vec2::new(0.0, top), Vec2::new(r, top), Vec2::new(r, bottom), Vec2::new(0.0, bottom - tip)];
                let region = Region2 { outer: solidcraft_geom::Loop2::polygon(&pts), holes: vec![] };
                return Ok(kernel::revolve(&rp, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)?
                    .into_iter()
                    .map(|tool| (crate::lineage::part_tool(tool, "bore"), None))
                    .collect());
            }
        }
        Ok(vec![cyl(r, bottom, top, "bore")?])
    };
    let mut tools = Vec::new();
    match hole {
        HoleKind::Simple => tools.extend(bore(None)?),
        HoleKind::Drilled { tip_angle } => tools.extend(bore(Some(tip_angle))?),
        HoleKind::Counterbore { cb_diameter, cb_depth, tip_angle } => {
            let (rc, dc) = (val(vals, cb_diameter, Kind::Length)? / 2.0, val(vals, cb_depth, Kind::Length)?);
            if !(rc > r && dc > 0.0 && -dc > bottom) {
                return Err(DocError::Invalid("the counterbore must be wider than the hole and shallower than it".into()));
            }
            // Wide and shallow first: the narrow hole then crosses the pocket floor.
            tools.push(cyl(rc, -dc, top, "cbore")?);
            tools.extend(bore(tip_angle.as_ref())?);
        }
        HoleKind::Countersink { cs_diameter, cs_angle, tip_angle } => {
            let (rc, half) = (val(vals, cs_diameter, Kind::Length)? / 2.0, val(vals, cs_angle, Kind::Angle)? / 2.0);
            if !(rc > r && half > 1e-3 && half < std::f64::consts::FRAC_PI_2) {
                return Err(DocError::Invalid("the countersink must be wider than the hole".into()));
            }
            // Cone from above the face to just inside the hole radius, then the hole.
            let r_top = rc + top * half.tan();
            let r_end = r * 0.9;
            let len = (r_top - r_end) / half.tan();
            let top_plane = plane.offset(top);
            tools.push((crate::lineage::part_tool(kernel::extrude_tapered(&top_plane, &disc(r_top), len, -1.0, -half)?, "csink"), None));
            tools.extend(bore(tip_angle.as_ref())?);
        }
    }
    Ok(tools)
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
    if !(sweep.is_finite() && start.is_finite() && sweep.abs() > 1e-9 && sweep.abs() <= std::f64::consts::TAU + 1e-9) {
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

/// The tool of an Extrude whose profile is a whole planar face of its one target (the face's
/// outline projected into the sketch), started at the face (#2040). The projection fits an edge
/// that is no line or arc, so the profile's tool lies a hair off the body all round its outline,
/// and the boolean fails there. A join takes the body's own face swept instead, which is sewn on
/// along it. A cut whose walls take nothing from the body (all of it within the depth lies over
/// the face) takes a slab beyond the depth plane instead, which meets the body only across that
/// plane; one whose walls cut into the body at some edges keeps those walls, exactly, and moves
/// the others off it (`kernel::cut_tool`). `None` (the profile's own tool) otherwise. With the
/// interval along the normal it spans.
fn whole_face_tool(
    st: &ModelState,
    targets: &[String],
    ss: &SolvedSketch,
    r: &Region2,
    operation: Operation,
    lo: f64,
    hi: f64,
) -> Option<(Traced, (f64, f64))> {
    if !matches!(operation, Operation::Join | Operation::Cut) || hi - lo <= 0.0 {
        return None;
    }
    // An outline of lines and arcs is the face's own: the profile's tool already meets the body
    // exactly, and keeps the names later features attach to (#489).
    if r.outer.segs.iter().all(|s| matches!(s, solidcraft_geom::Seg2::Line { .. } | solidcraft_geom::Seg2::Arc { .. })) {
        return None;
    }
    let mut candidates = st.bodies.iter().filter(|b| targets.is_empty() || targets.contains(&b.name));
    let target = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    let face = kernel::matching_face(&target.body, &ss.plane, r, &[lo, hi])?;
    let n = ss.plane.normal();
    let tol = target.body.size() * 1e-6 + 1e-9;
    let up = face.normal.dot(n) > 0.0;
    let (at_lo, at_hi) = ((face.height - lo).abs() <= tol, (face.height - hi).abs() <= tol);
    let roles = |region| crate::lineage::ProfileRoles {
        sketch: Some(ss),
        region: Some(region),
        caps: [Some(crate::lineage::extrude_cap_role(lo)), Some(crate::lineage::extrude_cap_role(hi))],
        thin: None,
    };
    match operation {
        // Out of the body from the face.
        Operation::Join if (up && at_lo) || (!up && at_hi) => {
            let built = centered_extrude(&ss.plane, r, lo, hi, Some(ss), None).ok()?;
            let [built] = built.as_slice() else { return None };
            let exact = kernel::face_prism(&target.body, face, hi - lo, true, &built.body).ok()?;
            kernel::can_join_touching_faces(&target.body, &exact).then(|| (roles(r).tool(exact, Some(&built.body)), (lo, hi)))
        }
        // Into the body from the face, its walls touching it at most.
        Operation::Cut if ((up && at_hi) || (!up && at_lo)) && !kernel::cut_takes_only_the_face(&target.body, face, hi - lo) => {
            // Some walls go into the body: those that only touch it are moved off.
            let built = centered_extrude(&ss.plane, r, lo, hi, Some(ss), None).ok()?;
            let [built] = built.as_slice() else { return None };
            let (tool, off) = kernel::cut_tool(&target.body, face, hi - lo, Some(&built.body))?;
            let span = if up { (lo, hi + off) } else { (lo - off, hi) };
            Some((roles(r).tool(tool, Some(&built.body)), span))
        }
        Operation::Cut if (up && at_hi) || (!up && at_lo) => {
            // Over the whole body, from the depth plane out past it.
            let reach = target.body.size() * 2.0 + (hi - lo);
            let c = r.outer.centroid();
            let p = |x: f64, y: f64| Vec2::new(c.x + x * reach, c.y + y * reach);
            let line = |a: Vec2, b: Vec2| solidcraft_geom::Seg2::Line { a, b };
            let corners = [p(-1.0, -1.0), p(1.0, -1.0), p(1.0, 1.0), p(-1.0, 1.0)];
            let segs = (0..4).filter_map(|i| Some(line(*corners.get(i)?, *corners.get((i + 1) % 4)?))).collect();
            let slab = Region2 { outer: solidcraft_geom::Loop2 { segs }, holes: vec![] };
            let (l, h) = if up { (lo, hi + reach) } else { (lo - reach, hi) };
            let built = centered_extrude(&ss.plane, &slab, l, h, None, None).ok()?;
            let [built] = built.as_slice() else { return None };
            #[cfg(test)]
            whole_face_tests::slab_taken();
            Some((roles(&slab).tool(built.body.clone(), None), (l, h)))
        }
        _ => None,
    }
}

/// Construct planar caps locally before placing the completed tool.
fn centered_extrude(
    plane: &Plane,
    region: &Region2,
    start: f64,
    end: f64,
    sketch: Option<&SolvedSketch>,
    thin: Option<&crate::lineage::ThinWallRoles>,
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
    let roles = crate::lineage::ProfileRoles {
        sketch,
        region: Some(region),
        caps: [Some(crate::lineage::extrude_cap_role(lo)), Some(crate::lineage::extrude_cap_role(hi))],
        thin,
    };
    // Fit cap surfaces in their own coordinates. Fitting a distant world-space
    // wire can tilt an otherwise exact planar cap through numeric cancellation.
    // A side drawn in collinear pieces is one face (#1788).
    let bodies = kernel::extrude_whole_sides(&Plane::XY, std::slice::from_ref(region), start, end)?;
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

/// The tool bodies a feature adds or removes (extrude, revolve, primitives), in the current state.
/// `to_body` is the body a To Object extent runs to (an extrude), when it found one.
fn feature_tools_with_constructor(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    constructor: &mut Option<ExtrudeConstructor>,
    to_body: &mut Option<String>,
) -> Result<Vec<Traced>> {
    *to_body = None;
    let unnamed = |bodies: Vec<Body>| -> Vec<Traced> { bodies.into_iter().map(Traced::unnamed).collect() };
    match &f.kind {
        FeatureKind::Extrude { sketch, profiles, extent, operation, targets, thin } => {
            let ss = sketch_in_feature_frame(
                doc,
                st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?,
                f.component,
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
            let tst =
                if extent.to.is_some() || extent.from.is_some() { state_in_frame(doc, st, f.component) } else { std::borrow::Cow::Borrowed(st) };
            // Through all, and where the tool meets its targets, is measured where they are shown.
            let shown = targets_as_shown(st, f, &crate::plain(targets))?;
            let st: &ModelState = &shown;
            // The face names follow `face_points`: To, then From.
            let name = |k: usize| f.face_names.get(k).map(String::as_str);
            let (to_name, from_name) = if extent.to.is_some() { (name(0), name(1)) } else { (None, name(0)) };
            let to_face = extent.to.and_then(|p| object_face(&tst, to_name, p.0, Some(n)));
            // The body the extent runs to: a Join to it joins that body too (#6424).
            *to_body = to_face.and_then(|(bi, _)| tst.bodies.get(bi)).map(|b| b.name.clone());
            let centre = {
                let pts: Vec<Vec2> = regions.iter().flat_map(|r| r.outer.polyline(1e-2)).collect();
                let c = pts.iter().fold(Vec2::default(), |a, p| a + *p) * (1.0 / pts.len().max(1) as f64);
                ss.plane.to_world(c)
            };
            let from = match extent.from {
                Some(p) => height_at(&tst, object_face(&tst, from_name, p.0, Some(n)), &ss.plane, centre, p.0),
                None => 0.0,
            };
            let d = match extent.to {
                Some(p) => {
                    let off = match &extent.to_offset {
                        Some(e) => val(vals, e, Kind::Length)?,
                        None => 0.0,
                    };
                    let h = height_at(&tst, to_face, &ss.plane, centre, p.0);
                    let start = from + extent.start_offset.as_deref().map(|e| val(vals, e, Kind::Length)).transpose()?.unwrap_or(0.0);
                    // Past the face by the offset, in the direction travelled.
                    let to = h + if h >= start { off } else { -off };
                    if (to - start).abs() < 1e-9 {
                        return Err(DocError::Invalid("the To object is where the extrude starts".into()));
                    }
                    to - start
                }
                None if extent.through_all => through_all_distance(st, &ss.plane, &crate::plain(targets))?,
                None => val(vals, &extent.distance, Kind::Length)?,
            };
            if let Some(tp) = &extent.taper {
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
                let start_plane = ss.plane.offset(base);
                let caps = [
                    Some(crate::lineage::extrude_cap_role(base)),
                    Some(crate::lineage::extrude_cap_role(if sign < 0.0 { base - len } else { base + len })),
                ];
                // How far a tool grows past its profile at either end, at most.
                let mut growth: f64 = 0.0;
                // A face of the targets in the profile's plane (the same for every profile).
                let plain_targets: Vec<String> = crate::plain(targets);
                let on_face = matches!(operation, Operation::Join | Operation::Cut) && face_level_at(st, &ss.plane, base, &plain_targets);
                let surroundings = Surroundings::of(st, *operation, &plain_targets);
                let tools: Vec<Traced> = regions
                    .iter()
                    .map(|r| {
                        // An end on a face of the target runs on into it, as a prism's does: tapered
                        // sides standing on a face are curved where they meet it (along text, say),
                        // and the boolean finds no result there. The sides keep their slope, so
                        // the profile stays the section at the start.
                        let roles =
                            crate::lineage::ProfileRoles { sketch: Some(&ss), region: Some(r), caps: caps.clone(), thin: thin_roles.as_ref() };
                        // A start that is a whole face of the target (a foot under a part's base) is sewn on
                        // along it, as it lies: run on, the sides would leave the face through its edges.
                        if on_face
                            && *operation == Operation::Join
                            && let Ok(exact) = kernel::extrude_tapered_between(&start_plane, r, 0.0, len, 0.0, sign, taper)
                            && st
                                .bodies
                                .iter()
                                .filter(|t| targets.is_empty() || targets.iter().any(|x| *x == t.name))
                                .any(|t| kernel::can_join_touching_faces(&t.body, &exact))
                        {
                            growth = growth.max(len * taper.tan().abs());
                            return Ok(roles.tool(exact, None));
                        }
                        let (lo, hi) = if sign < 0.0 { (base - len, base) } else { (base, base + len) };
                        let (l2, h2) = surroundings.as_ref().map_or((lo, hi), |sur| extend_for_coplanar(sur, &ss.plane, r, lo, hi));
                        let (mut a, b) = if sign < 0.0 { (base - h2, base - l2) } else { (l2 - base, h2 - base) };
                        // A taper that narrows the tool away from the face widens it as it runs on into it, past
                        // the profile (where the profile is the whole face, out through the body's sides, #1534):
                        // then it starts on the face instead.
                        if a < 0.0 && a * taper.tan() > 0.0 {
                            let dir = n * sign;
                            if !taper_run_on_fits(st, &start_plane, dir, r, -a, a * taper.tan(), *operation, &plain_targets) {
                                a = 0.0;
                            }
                        }
                        // A start that can't run on (the profile hangs over the face's edge):
                        // the sides stand straight for a hair first, so the boolean can push the
                        // face the tool stands on into the body.
                        let build = |a: f64| {
                            let foot = if a >= 0.0 && on_face { TAPER_FOOT.min((b - a) * 0.01) } else { 0.0 };
                            kernel::extrude_tapered_between(&start_plane, r, a, b, foot, sign, taper)
                        };
                        let mut body = build(a);
                        // A taper that widens the tool where it cuts narrows it behind its start, so
                        // the run on can close a small corner arc the tool itself never shrinks
                        // (#4825): run on less, then start on the face.
                        for k in [0.5, 0.25, 0.0] {
                            if body.is_ok() || a >= 0.0 {
                                break;
                            }
                            let shorter = build(a * k);
                            if shorter.is_ok() {
                                a *= k;
                                body = shorter;
                            }
                        }
                        let body = body?;
                        growth = growth.max(a.abs().max(b.abs()) * taper.tan().abs());
                        Ok(roles.tool(body, None))
                    })
                    .collect::<Result<_>>()?;
                // Profiles apart by more than the tools grow (the letters of a text): the tools
                // are apart too, and go into the body as one, in one boolean rather than one per
                // profile, each on a body grown or cut by the ones before. (A join meeting several
                // bodies keeps joining them tool by tool; sheet metal keeps cutting tool by tool.)
                let sheet =
                    if targets.is_empty() { !st.sheets.is_empty() } else { targets.iter().any(|t| st.sheets.iter().any(|sheet| &sheet.body == t)) };
                let one_target = if targets.is_empty() { st.bodies.len() == 1 } else { targets.len() == 1 };
                let at_once = match operation {
                    Operation::Cut => !sheet,
                    Operation::Join => one_target && !sheet,
                    _ => false,
                };
                if at_once && tools.len() > 1 && regions_apart(&regions, APART_GAP + 2.0 * growth) {
                    let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
                    if let Ok(Some(one)) = kernel::join_apart(&bodies) {
                        return Ok(vec![Traced::step(one, &tools.iter().collect::<Vec<_>>())]);
                    }
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
            // lies before its surface (the tool less the body, the piece at the start).
            let curved = curved_face(&tst, to_face, Some(n));
            if let Some(bi) = curved
                && let Some(tb) = tst.bodies.get(bi)
            {
                let m = tb.mesh();
                let hs: Vec<f64> = m.positions.iter().map(|q| (*q - ss.plane.origin).dot(n) - off).collect();
                let margin = tb.body.size() * 0.05 + 1.0;
                if hi > 0.0 {
                    hi = hs.iter().cloned().fold(hi, f64::max) + margin;
                } else {
                    lo = hs.iter().cloned().fold(lo, f64::min) - margin;
                }
            }
            let mut tools = Vec::new();
            let mut untrimmed = extent.to.is_none() && extent.from.is_none() && !extent.through_all;
            // The footprint of each tool in the sketch: a tool lies in the prism over it.
            let mut footprints = Vec::with_capacity(regions.len());
            // Each tool's extent along the normal (they differ where one is run on into a face).
            let mut extents: Vec<(f64, f64)> = Vec::with_capacity(regions.len());
            let plain_targets: Vec<String> = crate::plain(targets);
            let exact_planar_cut = *operation == Operation::Cut
                && ss.sketch.links.len() > 1
                && ss.sketch.links.iter().all(|link| matches!(&link.source, solidcraft_sketch::LinkSource::Face { .. }))
                && st
                    .bodies
                    .iter()
                    .filter(|body| plain_targets.is_empty() || plain_targets.iter().any(|target| target == "*" || target == &body.name))
                    .all(|body| kernel::is_planar(&body.body));
            let mut touching_batch = false;
            // Generated face profiles have full source seams. Check all of them in
            // one guarded construction; arbitrary sketch profiles retain their existing path.
            if *operation == Operation::Join
                && regions.len() > 1
                && ss.sketch.links.len() > 1
                && ss.sketch.links.iter().all(|link| matches!(&link.source, solidcraft_sketch::LinkSource::Face { .. }))
            {
                let exact = regions
                    .iter()
                    .map(|region| centered_extrude(&ss.plane, region, lo + off, hi + off, Some(&ss), thin_roles.as_ref()))
                    .collect::<Result<Vec<_>>>()?
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>();
                if let Ok(Some(compound)) = kernel::compound_disjoint_extrusions(&exact.iter().map(|tool| tool.body.clone()).collect::<Vec<_>>())
                    && st
                        .bodies
                        .iter()
                        .filter(|body| plain_targets.is_empty() || plain_targets.contains(&body.name))
                        .any(|body| kernel::can_join_all_touching_faces(&body.body, &compound))
                {
                    tools = exact;
                    touching_batch = true;
                }
            }
            let surroundings = if exact_planar_cut || touching_batch { None } else { Surroundings::of(st, *operation, &plain_targets) };
            let join_surroundings = if *operation == Operation::Join && !touching_batch {
                // Trimming may discard a tool's material, so only bodies this feature
                // actually joins can establish that the discarded end is already filled.
                let participants = if plain_targets.is_empty() { &f.participants } else { &plain_targets };
                let trim_targets: Vec<String> = st
                    .bodies
                    .iter()
                    .filter(|b| {
                        if participants.is_empty() {
                            b.component == f.component || b.feature == f.id
                        } else {
                            participants.iter().any(|t| t == "*" || t == &b.name)
                        }
                    })
                    .map(|b| b.name.clone())
                    .collect();

                if trim_targets.is_empty() { None } else { Surroundings::of(st, *operation, &trim_targets) }
            } else {
                None
            };
            for r in regions.iter().filter(|_| !touching_batch) {
                if regions.len() == 1
                    && let Some((tool, (l2, h2))) = whole_face_tool(st, &plain_targets, &ss, r, *operation, lo + off, hi + off)
                {
                    untrimmed &= l2 == lo + off && h2 == hi + off;
                    tools.push(tool);
                    footprints.push(r.clone());
                    extents.push((l2, h2));
                    continue;
                }
                if *operation == Operation::Join {
                    let exact = centered_extrude(&ss.plane, r, lo + off, hi + off, Some(&ss), thin_roles.as_ref())?;
                    if exact.iter().all(|tool| {
                        st.bodies
                            .iter()
                            .filter(|b| targets.is_empty() || targets.iter().any(|t| *t == b.name))
                            .any(|b| kernel::can_join_touching_faces(&b.body, &tool.body))
                    }) {
                        // Preserve a whole touching face: extending into a chamfer replaces
                        // the sewable seam with a coincident curved intersection.
                        tools.extend(exact);
                        footprints.push(r.clone());
                        extents.push((lo + off, hi + off));
                        continue;
                    }
                }
                let (l2, h2) = surroundings.as_ref().map_or((lo + off, hi + off), |sur| extend_for_coplanar(sur, &ss.plane, r, lo + off, hi + off));
                let (l2, h2) = join_surroundings.as_ref().map_or((l2, h2), |sur| {
                    // Trim only material in the requested extent. A run-on added
                    // above to resolve a touching cap must keep doing its job.
                    let (l, h) = trim_embedded_join_ends(sur, &ss.plane, r, lo + off, hi + off);
                    (if l > lo + off { l } else { l2 }, if h < hi + off { h } else { h2 })
                });
                let probe = |p: Vec2| (1..4).map(|k| ss.plane.to_world(p) + n * (lo + off + (hi - lo) * k as f64 / 4.0)).collect::<Vec<_>>();
                let original = r;
                let grown = |gains: &dyn Fn(&[[Vec2; 3]]) -> bool| match &surroundings {
                    Some(sur) => grow_profile_for_coplanar(sur, r, &probe, gains),
                    None => r.clone(),
                };
                // A hole of the profile over empty space (a cut) or material (a join) is taken in.
                let filled = |p: Region2| fill_void_holes(st, &p, *operation, &plain_targets, &ss.plane, (l2, h2));
                // A side's strip runs the tool's height, drawn back by a hair from its ends.
                let mut r = filled(grown(&|pieces| {
                    surroundings.as_ref().is_some_and(|sur| sur.harmless(&ss.plane, pieces, -sur.skin, l2 + sur.skin, h2 - sur.skin))
                }));
                let mut built = centered_extrude(&ss.plane, &r, l2, h2, Some(&ss), thin_roles.as_ref());
                if built.is_err() {
                    // A profile the kernel cannot build as drawn (a square some hundredths of a
                    // micrometre across, away from the origin, defeats truck's plane fit) grows
                    // wherever the samples find room, as it used to.
                    let loose = filled(grown(&|_| true));
                    if loose != r {
                        built = centered_extrude(&ss.plane, &loose, l2, h2, Some(&ss), thin_roles.as_ref());
                        r = loose;
                    }
                }
                untrimmed &= l2 == lo + off && h2 == hi + off && &r == original;
                tools.extend(built?);
                footprints.push(r);
                extents.push((l2, h2));
            }
            // A cut or a join by profiles apart in the sketch (the letters of a text): each tool
            // lies in the prism over its footprint, so the tools are apart too (whatever their
            // heights, and trimmed or not). The body is then cut once by all of them, or joined to
            // all of them at once, rather than once per profile, each on a body grown or cut by
            // the ones before. (A join meeting several bodies keeps joining them tool by tool;
            // sheet metal keeps its own way of cutting, tool by tool, to keep its flat pattern.)
            let sheet =
                if targets.is_empty() { !st.sheets.is_empty() } else { targets.iter().any(|t| st.sheets.iter().any(|sheet| &sheet.body == t)) };
            let one_target = if targets.is_empty() { st.bodies.len() == 1 } else { targets.len() == 1 };
            // A join at once only of tools alike in height (one prism of them all, which the
            // boolean joins exactly): tools that start at different levels (some run on into the
            // face, one hanging over its edge) keep joining one by one.
            let alike = extents.windows(2).all(|w| (w[0].0 - w[1].0).abs() < 1e-9 && (w[0].1 - w[1].1).abs() < 1e-9);
            let at_once = match operation {
                Operation::Cut => !sheet,
                Operation::Join => one_target && !sheet && alike,
                _ => false,
            };
            if at_once && tools.len() > 1 && regions_apart(&footprints, APART_GAP) {
                let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
                if let Ok(Some(one)) = kernel::join_apart(&bodies) {
                    tools = vec![Traced::step(one, &tools.iter().collect::<Vec<_>>())];
                }
            }
            if let Some(tb) = curved.and_then(|bi| tst.bodies.get(bi)) {
                let start = if hi > 0.0 { lo + off } else { hi + off };
                tools = trim_to_body(tools, tb, |q: Vec3| ((q - ss.plane.origin).dot(n) - start).abs())?;
            }
            if untrimmed {
                *constructor = Some(ExtrudeConstructor { plane: ss.plane, interval: (lo + off, hi + off) });
            }
            // A Join tool's faces outlive the boolean where they stay on the result, so they are
            // certified as a new body's are (see `Body::with_preserved_union_planar_sources`).
            if matches!(operation, Operation::NewBody | Operation::Join) && extent.taper.is_none() {
                let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
                let cap_bindings = match thin {
                    Some(thin) => Some(crate::config::thin_extrude_cap_bindings(&thin.curves, regions.len(), &bodies)),
                    None => crate::config::extrude_cap_bindings(&ss, &bodies, 10_000_000),
                };
                let source_curves = cap_bindings.as_ref().map(|_| crate::config::ExactSketchCurves::new(&ss));
                tools = tools
                    .into_iter()
                    .enumerate()
                    .map(|(body_index, Traced { body, lineage })| {
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
                        let nominal = |plane: &solidcraft_geom::Plane| {
                            let height = (plane.origin - ss.plane.origin).dot(n);
                            let tolerance = 1e-9 * (1.0 + height.abs() + (lo + off).abs() + (hi + off).abs());
                            (height - (lo + off)).abs() <= tolerance || (height - (hi + off)).abs() <= tolerance
                        };
                        for (cap, source) in caps.iter_mut().zip(body.face_sources().into_iter().flatten()) {
                            if matches!(source, Some(kernel::FaceSource::ExtrudeCap { plane, .. }) if !nominal(plane)) {
                                *cap = None;
                            }
                        }
                        // Certificates set face sources only: the faces, and their lineage, stay.
                        let body = body.with_extrude_certificates(f.id, sketch.0, &curves)?.with_extrude_cap_certificates(f.id, sketch.0, &caps)?;
                        Ok(Traced { body, lineage })
                    })
                    .collect::<Result<Vec<_>>>()?;
            }
            Ok(tools)
        }
        FeatureKind::Revolve { sketch, profiles, axis, angle, angle2, symmetric, convention, to, .. } => {
            let ss = sketch_in_feature_frame(
                doc,
                st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?,
                f.component,
            )?;
            let regions = solidcraft_sketch::merge_regions(&select_profiles(&ss, profiles)?);
            let (o, d) =
                revolve_axis(&ss, axis, &axis_state(doc, st, f.component, axis), &|name| construction_axis_in(Some(doc), st, name, f.component))?;
            // To Object is in this feature's component frame: the model as seen from there.
            let tst = if to.is_some() { state_in_frame(doc, st, f.component) } else { std::borrow::Cow::Borrowed(st) };
            // Where the tool meets its targets is measured where they are shown (#3383).
            let shown = targets_as_shown(st, f, &feature_op(f).1)?;
            let st: &ModelState = &shown;
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
            let (operation, targets) = feature_op(f);
            let axis_w = ss.plane.dir_to_world(d);
            let origin_w = ss.plane.to_world(o);
            let surroundings = Surroundings::of(st, operation, &targets);
            let regions: Vec<Region2> = regions
                .iter()
                .zip(&region_extents)
                .map(|(r, region_extent)| {
                    let rotation_axis = match convention {
                        crate::RevolveConvention::LegacyAxisStart => axis_w,
                        crate::RevolveConvention::SketchNormal => kernel::canonical_revolve_axis(&ss.plane, r, o, d)?,
                    };
                    let probe = |p: Vec2| {
                        let q = ss.plane.to_world(p);
                        (1..4)
                            .map(|k| {
                                let a = region_extent.sweep * k as f64 / 4.0
                                    + if *convention == crate::RevolveConvention::SketchNormal { region_extent.start } else { 0.0 };
                                let v = q - origin_w;
                                origin_w + v * a.cos() + rotation_axis.cross(v) * a.sin() + rotation_axis * (rotation_axis.dot(v) * (1.0 - a.cos()))
                            })
                            .collect::<Vec<_>>()
                    };
                    // Around the axis the strips are rings, which the faces are not asked about yet.
                    Ok(match &surroundings {
                        Some(sur) => grow_profile_for_coplanar(sur, r, &probe, &|_| true),
                        None => r.clone(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
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
                    let m = crate::rigid(Vec3::ZERO, origin_w, rotation_axis, region_extent.start);
                    for body in bodies {
                        out.push(if region_extent.start.abs() < 1e-12 {
                            tool(region, body, None)
                        } else {
                            tool(region, kernel::transform_matrix(&body, m)?, Some(&body))
                        });
                    }
                }
                return Ok(out);
            }
            let bodies = revolve(&ss.plane, &regions, o, d, ang)?;
            if start.abs() < 1e-12 {
                return Ok(regions.iter().zip(bodies).map(|(r, body)| tool(r, body, None)).collect());
            }
            // Turned back to start before the profile.
            let m = crate::rigid(Vec3::ZERO, origin_w, axis_w, -start);
            regions.iter().zip(bodies).map(|(r, body)| Ok(tool(r, kernel::transform_matrix(&body, m)?, Some(&body)))).collect()
        }
        FeatureKind::Loft { sections, rails, .. } if sections.iter().any(|s| s.point.is_some()) => {
            // A profile lofted to a sketch point (an apex).
            if !rails.is_empty() {
                return Err(DocError::Invalid("not supported yet: guide rails on a loft to a point".into()));
            }
            let [a, b] = &sections[..] else { return Err(DocError::Invalid("not supported yet: a point section in a loft of more than two".into())) };
            let (prof, pt) = if a.point.is_some() { (b, a) } else { (a, b) };
            let ss = st.sketch(prof.sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {}", prof.sketch)))?;
            let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, &prof.profiles)?);
            let [r] = &regions[..] else { return Err(DocError::Invalid("each loft section must be one profile".into())) };
            refuse_loft_holes(r)?;
            let ps = st.sketch(pt.sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {}", pt.sketch)))?;
            let id = pt.point.as_deref().unwrap_or_default();
            let q = ps.sketch.points.iter().find(|p| p.id == id).ok_or_else(|| DocError::Unknown(format!("point `{id}` in {}", ps.name)))?;
            let body = kernel::loft_to_point(&ss.plane, &r.outer, ps.plane.to_world(q.pos))?;
            // Its sides are named by the profile's curves, its base `start`.
            Ok(vec![crate::lineage::ProfileRoles::swept(Some(ss), Some(r)).tool(body, None)])
        }
        FeatureKind::Coil {
            base, axis, diameter, pitch, turns, section_size, section, position, start_angle, clockwise, sketch, profiles, ..
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
                    || *position != 0)
            {
                return Err(DocError::Invalid(
                    "a coil with a `sketch` cannot also set `diameter`, `section_size`, `section`, `section_position` or `start_angle`".into(),
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
            let plane = Plane { origin: *base, x, y: k };
            let c = Vec2::new(r, 0.0);
            let outer = match section {
                crate::CoilSection::Circular => solidcraft_geom::Loop2::circle(c, s / 2.0),
                crate::CoilSection::Square => {
                    let h = s / 2.0;
                    solidcraft_geom::Loop2::polygon(&[c + Vec2::new(-h, -h), c + Vec2::new(h, -h), c + Vec2::new(h, h), c + Vec2::new(-h, h)])
                }
            };
            let region = Region2 { outer, holes: Vec::new() };
            let axis = if *clockwise { k * -1.0 } else { k };
            // A left-handed coil turns about −axis and climbs along +axis: mirror the pitch.
            let pitch = if *clockwise { -p } else { p };
            Ok(unnamed(vec![kernel::sweep_helix(&plane, &region, *base, axis, pitch, n)?]))
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
        FeatureKind::Sweep { sketch, profiles, path_sketch, path, orientation, twist, scale, rails, rail_fit, .. } => {
            let ss = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch} (it must come earlier in the timeline)")))?;
            let ps =
                st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
            let profile;
            let ss = if !ps.sketch.wires.is_empty() {
                profile = profile_in_frame(doc, ss, f.component)?;
                &profile
            } else {
                ss
            };
            let mapped = path_in_frame(doc, ps, f.component)?;
            let ps = mapped.as_ref();
            let regions = solidcraft_sketch::merge_regions(&select_profiles(ss, profiles)?);
            let start = regions.first().map(|r| ss.plane.to_world(r.centroid())).unwrap_or_default();
            let tool = |r: &Region2, body: Body| crate::lineage::ProfileRoles::swept(Some(ss), Some(r)).tool(body, None);
            let points = path_points(ps, path, start)?;
            let segs = match points {
                Some(_) => Vec::new(),
                None => path_segments(ps, path, start)?,
            };
            let path_start = points
                .as_ref()
                .and_then(|p| p.first().copied())
                .or_else(|| match segs.first() {
                    Some(kernel::PathSeg::Line { a, .. } | kernel::PathSeg::Arc { a, .. }) => Some(*a),
                    None => None,
                })
                .unwrap_or(start);
            let rails = rail_polylines(doc, st, rails, f.component, path_start)?;
            let ctl = sweep_control(vals, orientation, twist, scale, rails, *rail_fit)?;
            // A path through 3D sketch curves follows them as points.
            if let Some(pts) = points {
                return regions.iter().map(|r| Ok(tool(r, kernel::sweep_path_with(&ss.plane, r, &pts, &ctl)?))).collect();
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
            if let Some(pts) = path_points(ps, path, Vec3::new(f64::NAN, f64::NAN, f64::NAN))? {
                let (Some(a), Some(b)) = (pts.first().copied(), pts.get(1).copied()) else { return Err(DocError::Invalid("empty path".into())) };
                let t = (b - a).normalized().ok_or_else(|| DocError::Invalid("path direction".into()))?;
                let plane = Plane::from_normal(a, t).ok_or_else(|| DocError::Invalid("pipe profile plane".into()))?;
                return Ok(unnamed(vec![kernel::sweep_path(&plane, &pipe_region(vals, r, wall)?, &pts)?]));
            }
            let segs = path_segments(ps, path, Vec3::new(f64::NAN, f64::NAN, f64::NAN))?;
            let first = segs.first().ok_or_else(|| DocError::Invalid("empty path".into()))?;
            let (start, tangent) = match *first {
                kernel::PathSeg::Line { a, b } => (a, b - a),
                kernel::PathSeg::Arc { a, center, axis, .. } => (a, axis.cross(a - center)),
            };
            let t = tangent.normalized().ok_or_else(|| DocError::Invalid("path direction".into()))?;
            let plane = Plane::from_normal(start, t).ok_or_else(|| DocError::Invalid("pipe profile plane".into()))?;
            Ok(unnamed(vec![kernel::sweep(&plane, &pipe_region(vals, r, wall)?, &segs)?]))
        }
        FeatureKind::Box { corner, length, width, height, .. } => {
            let s = Vec3::new(val(vals, length, Kind::Length)?, val(vals, width, Kind::Length)?, val(vals, height, Kind::Length)?);
            let corner = point(vals, corner)?;
            Ok(vec![crate::lineage::box_tool(kernel::box_solid(corner, corner + s)?)])
        }
        FeatureKind::Cylinder { base, axis, radius, height, .. } => Ok(vec![crate::lineage::cylinder_tool(kernel::cylinder(
            cylinder_base(vals, st, f, base, axis.0)?,
            axis.0,
            val(vals, radius, Kind::Length)?,
            val(vals, height, Kind::Length)?,
        )?)]),
        FeatureKind::Sphere { center, radius, .. } => Ok(unnamed(vec![kernel::sphere(point(vals, center)?, val(vals, radius, Kind::Length)?)?])),
        FeatureKind::Torus { center, major, minor, .. } => {
            Ok(unnamed(vec![kernel::torus(point(vals, center)?, val(vals, major, Kind::Length)?, val(vals, minor, Kind::Length)?)?]))
        }
        FeatureKind::Cone { base, axis, radius, top_radius, height, .. } => Ok(unnamed(vec![more::cone(
            point(vals, base)?,
            axis.0,
            val(vals, radius, Kind::Length)?,
            val(vals, top_radius, Kind::Length)?,
            val(vals, height, Kind::Length)?,
        )?])),
        FeatureKind::Hole { .. } => Ok(hole_feature_tools(vals, st, f)?.into_iter().map(|(tool, _)| tool).collect()),
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
            match path_points(rs, &rail.curves, start)? {
                Some(pts) => Ok(pts),
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
            vec![solidcraft_geom::Loop2::circle(Vec2::ZERO, r - wt).reversed()]
        }
        None => Vec::new(),
    };
    Ok(Region2 { outer: solidcraft_geom::Loop2::circle(Vec2::ZERO, r), holes })
}

pub fn map_wire(wire: &mut solidcraft_sketch::Wire, matrix: &Mat) {
    for point in wire.pts.iter_mut().chain(&mut wire.fit) {
        *point = crate::apply_point(matrix, *point);
    }
    if let Some(arc) = &mut wire.arc {
        let normal = crate::apply_vector(matrix, arc.normal);
        arc.center = crate::apply_point(matrix, arc.center);
        arc.start = crate::apply_point(matrix, arc.start);
        arc.end = crate::apply_point(matrix, arc.end);
        arc.normal = normal.normalized().unwrap_or(arc.normal);
        arc.radius = arc.center.dist(arc.start);
    }
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

fn profile_in_frame(doc: &Document, sketch: &SolvedSketch, consumer: u64) -> Result<SolvedSketch> {
    let owner = doc.feature(sketch.feature).map(|feature| feature.component).unwrap_or(0);
    let inverse =
        crate::mat_inverse(&doc.component_transform(consumer)).ok_or_else(|| DocError::Invalid("profile consumer frame is invalid".into()))?;
    let relative = crate::mat_mul(&inverse, &doc.component_transform(owner));
    let mut mapped = sketch.clone();
    mapped.plane = Plane::new(
        crate::apply_point(&relative, sketch.plane.origin),
        crate::apply_vector(&relative, sketch.plane.x),
        crate::apply_vector(&relative, sketch.plane.y),
    )
    .ok_or_else(|| DocError::Invalid("profile sketch frame is invalid".into()))?;
    Ok(mapped)
}

/// Stored wire coordinates are converted once into the feature consumer frame.
fn path_in_frame<'a>(doc: &Document, sketch: &'a SolvedSketch, consumer: u64) -> Result<std::borrow::Cow<'a, SolvedSketch>> {
    use std::borrow::Cow;
    if sketch.sketch.wires.is_empty() {
        return Ok(Cow::Borrowed(sketch));
    }
    let owner = doc.feature(sketch.feature).map(|f| f.component).unwrap_or(0);
    let inverse =
        crate::mat_inverse(&doc.component_transform(consumer)).ok_or_else(|| DocError::Invalid("path consumer frame is not invertible".into()))?;
    let relative = crate::mat_mul(&inverse, &doc.component_transform(owner));
    let mut mapped = sketch.clone();
    mapped.plane = Plane::new(
        crate::apply_point(&relative, sketch.plane.origin),
        crate::apply_vector(&relative, sketch.plane.x),
        crate::apply_vector(&relative, sketch.plane.y),
    )
    .ok_or_else(|| DocError::Invalid("path sketch frame is invalid".into()))?;
    for wire in &mut mapped.sketch.wires {
        let matrix = if wire.space == solidcraft_sketch::WireSpace::Component { &relative } else { &inverse };
        map_wire(wire, matrix);
    }
    Ok(Cow::Owned(mapped))
}

/// A path that uses 3D sketch curves (wires), as one chain of world points from the end nearest
/// `start` (no start: the first curve runs toward the second). `None` when the path is all
/// planar curves.
fn path_points(ps: &SolvedSketch, ids: &[String], start: Vec3) -> Result<Option<Vec<Vec3>>> {
    if !ids.iter().any(|id| ps.sketch.wire_index(id).is_some()) {
        return Ok(None);
    }
    if ids.len() > 1000 {
        return Err(DocError::Invalid("the path needs 1…1000 curves".into()));
    }
    let mut pieces: Vec<Vec<Vec3>> = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(wi) = ps.sketch.wire_index(id) {
            pieces.push(ps.sketch.wires.get(wi).map(|w| w.pts.clone()).unwrap_or_default());
        } else {
            let ci = ps.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("path curve `{id}`")))?;
            let mut pts: Vec<Vec3> = Vec::new();
            for s in ps.sketch.segs(ci) {
                for q in s.polyline(1e-2) {
                    let w = ps.plane.to_world(q);
                    if pts.last().is_none_or(|l| l.dist(w) > 1e-9) {
                        pts.push(w);
                    }
                }
            }
            pieces.push(pts);
        }
    }
    // Pieces meet within the sketch's tolerance (projected curves are only that close).
    let tol = 1e-3;
    let ends = |p: &Vec<Vec3>| (p.first().copied().unwrap_or_default(), p.last().copied().unwrap_or_default());
    if let Some(f) = pieces.first().cloned() {
        let (a, b) = ends(&f);
        let flip = if start.is_finite() {
            b.dist(start) < a.dist(start)
        } else {
            pieces.get(1).is_some_and(|n| {
                let (na, nb) = ends(n);
                na.dist(a).min(nb.dist(a)) < na.dist(b).min(nb.dist(b))
            })
        };
        if flip && let Some(p0) = pieces.first_mut() {
            p0.reverse();
        }
    }
    let mut chain: Vec<Vec3> = Vec::new();
    for mut p in pieces {
        if let Some(last) = chain.last().copied() {
            let (a, b) = ends(&p);
            if b.dist(last) < a.dist(last) {
                p.reverse();
            }
            if p.first().is_some_and(|a| a.dist(last) > tol) {
                return Err(DocError::Invalid("the path curves are not connected end to end".into()));
            }
            chain.extend(p.into_iter().skip(1));
        } else {
            chain = p;
        }
    }
    Ok(Some(chain))
}

fn path_segments(ps: &SolvedSketch, ids: &[String], start: Vec3) -> Result<Vec<kernel::PathSeg>> {
    if ids.is_empty() || ids.len() > 1000 {
        return Err(DocError::Invalid("the path needs 1…1000 curves".into()));
    }
    // A path through 3D sketch curves is followed as a 3D polyline.
    if let Some(pts) = path_points(ps, ids, start)? {
        return Ok(pts.windows(2).map(|w| kernel::PathSeg::Line { a: w[0], b: w[1] }).collect());
    }
    let mut segs: Vec<solidcraft_geom::Seg2> = Vec::new();
    for id in ids {
        let ci = ps.sketch.curve_index(id).ok_or_else(|| DocError::Unknown(format!("path curve `{id}`")))?;
        segs.extend(ps.sketch.segs(ci));
    }
    // Orient the chain: the first segment starts at the end nearest `start`, each next one
    // starts where the previous ended.
    let w = |p: Vec2| ps.plane.to_world(p);
    if let (Some(f), true) = (segs.first().copied(), !segs.is_empty())
        && w(f.end()).dist(start) < w(f.start()).dist(start)
        && (segs.len() == 1 || segs.get(1).is_some_and(|n| n.start().dist(f.start()) < 1e-6 || n.end().dist(f.start()) < 1e-6))
    {
        segs[0] = f.reversed();
    }
    // No start given (a pipe): the first curve runs toward the second.
    if !start.is_finite()
        && let (Some(f), Some(n)) = (segs.first().copied(), segs.get(1).copied())
        && (n.start().dist(f.start()) < 1e-6 || n.end().dist(f.start()) < 1e-6)
    {
        segs[0] = f.reversed();
    }
    for i in 1..segs.len() {
        let prev_end = segs[i - 1].end();
        if segs[i].end().dist(prev_end) < segs[i].start().dist(prev_end) {
            segs[i] = segs[i].reversed();
        }
        if segs[i].start().dist(prev_end) > 1e-5 {
            return Err(DocError::Invalid("the path curves are not connected end to end".into()));
        }
    }
    let n = ps.plane.normal();
    Ok(segs
        .iter()
        .flat_map(|s| match *s {
            solidcraft_geom::Seg2::Line { a, b } => vec![kernel::PathSeg::Line { a: w(a), b: w(b) }],
            solidcraft_geom::Seg2::Arc { center, sweep, .. } => {
                vec![kernel::PathSeg::Arc { a: w(s.start()), center: w(center), axis: n * sweep.signum(), angle: sweep.abs() }]
            }
            // Free-form path curves are followed as polylines.
            solidcraft_geom::Seg2::Cubic { .. } | solidcraft_geom::Seg2::Conic { .. } | solidcraft_geom::Seg2::Bezier { .. } => {
                s.polyline(1e-2).windows(2).map(|q| kernel::PathSeg::Line { a: w(q[0]), b: w(q[1]) }).collect()
            }
        })
        .collect())
}

/// Where an extrude of distance `d` starts and ends along the sketch normal, before any start
/// offset: one side, the other, both (symmetric), or a second distance on the other side.
///
/// A second distance (`distance2`) goes the other way from the first side, wherever that went:
/// against the normal for `direction: negative` (#2700), and back along it when a negative
/// distance or a To Object below the sketch sends the first side down.
pub(crate) fn extent_sides(vals: &BTreeMap<String, Value>, extent: &crate::Extent, d: f64, direction: crate::Direction) -> Result<(f64, f64)> {
    Ok(match (&extent.distance2, direction) {
        (Some(d2), _) => {
            let first = if direction == crate::Direction::Negative { -d } else { d };
            let d2 = val(vals, d2, Kind::Length)?;
            (if first < 0.0 { d2 } else { -d2 }, first)
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

/// A copy of a sheet metal body is sheet metal too: its sheet model goes with it, moved, or
/// mirrored into the opposite hand ([`crate::sheet::SheetBody::copied`]), so the copy has its
/// own flat pattern, DXF and bend table. A copy by any other transform stays a plain solid.
fn copy_sheet(st: &mut ModelState, from: &str, to: &str, m: &crate::Mat) {
    let Some(sheet) = st.sheets.iter().find(|s| s.body == from).and_then(|s| s.copied(to, m)) else { return };
    st.sheets.retain(|s| s.body != to);
    st.sheets.push(sheet);
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

const MAX_INSTANCES: usize = 1000;

/// Where a mirrored body was picked and where its copy goes: the component the copy joins and
/// that component's world frame, and the frame the body's own geometry is in where it was picked
/// (its placement `path`, with its offset). `None` in older designs, which saved no placement and
/// mirror the body's own geometry.
fn mirrored_placement(doc: &Document, mb: &ModelBody, path: Option<&[u64]>) -> Result<Option<(u64, Mat, Mat)>> {
    let Some(path) = path else { return Ok(None) };
    let lost = || DocError::Invalid(format!("the placement `{}` was mirrored at no longer exists", mb.name));
    // A stale or edited path that leads to another component no longer shows the body.
    let (at, _) = doc.path_placement(path).ok_or_else(lost)?;
    if !placement_shows(doc, mb, at) {
        return Err(lost());
    }
    let (component, frame, picked) = mirror_copy_frame(doc, path).ok_or_else(lost)?;
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
/// insert can reuse the name): the component its copy joins, that component's world frame, and
/// the placement's world frame. The copy joins the body's component, or, when that component is
/// placed more than once (a copy there would show at every placement), the innermost component
/// around it that is placed once.
fn mirror_copy_frame(doc: &Document, path: &[u64]) -> Option<(u64, Mat, Mat)> {
    let (_, picked) = doc.path_placement(path)?;
    let placed_once = |id: &u64| {
        let component = doc.occurrences.iter().find(|o| o.id == *id).map(|o| o.component);
        doc.occurrences.iter().filter(|o| Some(o.component) == component).count() == 1
    };
    let once = path.iter().take_while(|id| placed_once(id)).count();
    let (component, frame) = doc.path_placement(path.get(..once)?)?;
    Some((component, frame, picked))
}

/// The placements (column-major matrices) of a pattern's copies, the original excluded.
pub fn pattern_matrices(doc: &Document, st: &ModelState, p: &crate::PatternKind) -> Result<Vec<[[f64; 4]; 4]>> {
    let (vals, _) = doc.param_values();
    pattern_transforms(doc, &vals, st, 0, p)
}

fn pattern_transforms(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, component: u64, p: &crate::PatternKind) -> Result<Vec<Mat>> {
    let count = |e: &str| -> Result<usize> {
        let n = expr::eval_count(e, vals)?;
        if !(1.0..=MAX_INSTANCES as f64).contains(&n) {
            return Err(DocError::Invalid(format!("pattern count must be 1…{MAX_INSTANCES}")));
        }
        Ok(n as usize)
    };
    let mut out = Vec::new();
    match p {
        crate::PatternKind::Rectangular { dir1, count1, spacing1, dir2, count2, spacing2, alignment, alignment2, reversed, reversed2 } => {
            let (n1, s1) = (count(count1)?, val(vals, spacing1, Kind::Length)?);
            let d1 = dir1.normalized().ok_or_else(|| DocError::Invalid("pattern direction".into()))?;
            let (n2, s2, d2) = match (dir2, count2, spacing2) {
                (Some(d), Some(c), Some(sp)) => {
                    (count(c)?, val(vals, sp, Kind::Length)?, d.normalized().ok_or_else(|| DocError::Invalid("pattern direction".into()))?)
                }
                _ => (1, 0.0, Vec3::Y),
            };
            if n1 * n2 > MAX_INSTANCES {
                return Err(DocError::Invalid("too many pattern instances".into()));
            }
            for i in 0..n1 {
                for j in 0..n2 {
                    if i == 0 && j == 0 {
                        continue;
                    }
                    out.push(translation(d1 * (s1 * alignment.multiplier(i, *reversed)) + d2 * (s2 * alignment2.multiplier(j, *reversed2))));
                }
            }
        }
        crate::PatternKind::Path { path_sketch, path, count: c, spacing, extent, flip, orient } => {
            let n = count(c)?;
            let d = val(vals, spacing, Kind::Length)?;
            let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("path sketch {path_sketch}")))?;
            let mapped = path_in_frame(doc, ps, component)?;
            let ps = mapped.as_ref();
            let mut segs = path_segments(ps, path, Vec3::new(f64::NAN, f64::NAN, f64::NAN))?;
            if *flip {
                segs = segs.iter().rev().map(reverse_seg).collect();
            }
            let total: f64 = segs.iter().map(seg_len).sum();
            let step = if *extent { if n > 1 { d / (n - 1) as f64 } else { 0.0 } } else { d };
            if (step * (n - 1) as f64).abs() > total * (1.0 + 1e-5) + 1e-6 {
                return Err(DocError::Invalid(format!("the instances run past the end of the path ({total:.3} mm long)")));
            }
            let normal = ps.plane.normal();
            let (p0, t0) = path_at(&segs, 0.0);
            for i in 1..n {
                let (pk, tk) = path_at(&segs, step * i as f64);
                let mut m = if *orient {
                    // Signed turn of the tangent about the sketch normal.
                    let ang = t0.cross(tk).dot(normal).atan2(t0.dot(tk));
                    rotation(p0, normal, ang)
                } else {
                    translation(Vec3::ZERO)
                };
                let dv = pk - p0;
                m[3][0] += dv.x;
                m[3][1] += dv.y;
                m[3][2] += dv.z;
                out.push(m);
            }
        }
        crate::PatternKind::Circular { origin, axis, count: c, angle, alignment, reversed, axis_ref, axis_feature } => {
            // On an edge or round face: about it as it is now; on a construction axis: where it is.
            let (origin, axis) = match (axis_ref, axis_feature) {
                (Some(g), _) => crate::construct::model_axis(&construction_state(doc, st, component), g)?,
                (None, Some(name)) => construction_axis_in(Some(doc), st, name, component)?,
                (None, None) => (origin.0, axis.0),
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
    }
}

fn reverse_seg(s: &kernel::PathSeg) -> kernel::PathSeg {
    match *s {
        kernel::PathSeg::Line { a, b } => kernel::PathSeg::Line { a: b, b: a },
        kernel::PathSeg::Arc { a, center, axis, angle } => {
            let k = axis.normalized().unwrap_or(Vec3::Z);
            let end = center + rotate_vec(a - center, k, angle);
            kernel::PathSeg::Arc { a: end, center, axis: k * -1.0, angle }
        }
    }
}

/// Point and tangent at arc length `d` along a chain (clamped to its ends).
fn path_at(segs: &[kernel::PathSeg], d: f64) -> (Vec3, Vec3) {
    let mut left = d.max(0.0);
    for (i, s) in segs.iter().enumerate() {
        let l = seg_len(s);
        if left <= l + 1e-9 || i + 1 == segs.len() {
            return seg_at(s, left.min(l));
        }
        left -= l;
    }
    (Vec3::ZERO, Vec3::X)
}

/// Whether a copy has no material to cut in any of its eligible targets.
fn copy_misses(st: &ModelState, f: &Feature, targets: &[String], tools: &[Traced]) -> bool {
    let targets = if targets.is_empty() { &f.participants } else { targets };
    let all = targets.len() == 1 && targets.first().is_some_and(|t| t == "*");
    st.bodies
        .iter()
        .filter(|b| all || if targets.is_empty() { b.component == f.component || b.feature == f.id } else { targets.contains(&b.name) })
        .all(|b| {
            tools.iter().all(|t| {
                // A diagnostic must neither reject an otherwise valid cut nor add its own
                // retries to the feature's report. An inconclusive probe is not a miss.
                let (intersection, _) = kernel::capture_boolean_diagnostics(|| kernel::boolean(&b.body, &t.body, BoolOp::Intersect));
                matches!(intersection, Ok(None))
            })
        })
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
        FeatureKind::Pattern { features, pattern, bodies } if bodies.is_empty() => {
            Ok(Some((features.clone(), pattern_transforms(doc, vals, st, src.component, pattern)?)))
        }
        FeatureKind::Mirror { features, plane, bodies, .. } if bodies.is_empty() => {
            let pl = doc.resolve_plane_in(vals, plane, 0, Some(st))?;
            Ok(Some((features.clone(), vec![mirror_matrix(&pl)])))
        }
        _ => Ok(None),
    }
}

/// The copies a pattern or mirror of `features` at `mats` makes, source by source: the source
/// feature, the transforms of its copies, and the instance number of its first copy less one.
/// A source that is itself a pattern or mirror of features (#395) stands for what it copied: its
/// own sources, at each of its transforms moved by each of ours. Those copies are numbered after
/// `mats`' own (which keep the numbers they always had), each nested source in a block of its
/// own, so no two copies of one feature share an instance. Suppressed sources copy nothing.
/// `f` with each face point moved onto its named face (on the body the feature names, when it
/// names one and the face is there) in `state`, and notes for names that no longer find their
/// face. `None` when `f` names no face.
fn with_named_face_points(doc: &Document, state: &ModelState, f: &Feature) -> Option<(Feature, Vec<String>)> {
    // Cylindrical emboss resolves its named face in the selected body's frame
    // and certifies an analytic anchor; mesh projection here would undo that.
    let explicit_emboss = matches!(&f.kind, FeatureKind::Emboss { face: Some(_), .. });
    if explicit_emboss || !f.face_names.iter().any(|n| !n.is_empty()) {
        return None;
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
    let fst = face_point_state(doc, state, &f.kind, f.component);
    for (k, (p, n)) in g.kind.face_points_mut().into_iter().zip(&f.face_names).enumerate() {
        if n.is_empty() {
            continue;
        }
        match crate::name_lookup::locate_face_on_body(&fst, f.kind.face_body(), n, *p) {
            Some(hit) => {
                if let Some(note) = hit.note(n) {
                    notes.push(note);
                }
                match (k, hole_axis) {
                    (0, Some(axis)) => match hit.along(&fst, *p, axis) {
                        Some(q) => *p = q,
                        // The place still lies on the face (the axis may run along it).
                        None if hit.on_point => {}
                        None => notes.push(format!(
                            "the hole's position [{}, {}, {}] is no longer on face `{n}`: it stays where it was placed",
                            p.x, p.y, p.z
                        )),
                    },
                    _ => *p = hit.point,
                }
            }
            None => notes.push(format!("face `{n}` no longer exists; using the face at the picked point")),
        }
    }
    Some((g, notes))
}

fn copy_plan<'a>(
    doc: &'a Document,
    vals: &BTreeMap<String, Value>,
    st: &ModelState,
    features: &[FeatureId],
    mats: &[Mat],
) -> Result<Vec<(&'a Feature, Vec<Mat>, usize)>> {
    #[allow(clippy::too_many_arguments)]
    fn walk<'a>(
        doc: &'a Document,
        vals: &BTreeMap<String, Value>,
        st: &ModelState,
        features: &[FeatureId],
        mats: &[Mat],
        first: usize,
        next: &mut usize,
        depth: usize,
        out: &mut Vec<(&'a Feature, Vec<Mat>, usize)>,
    ) -> Result<()> {
        for id in features {
            if doc.feature(id.0).is_some_and(|x| x.suppressed) {
                continue;
            }
            let src = source_feature(doc, *id)?;
            let Some((inner, inner_mats)) = copied_features(doc, vals, st, src)? else {
                out.push((src, mats.to_vec(), first));
                continue;
            };
            if depth >= MAX_NESTING {
                return Err(DocError::Invalid(format!("patterns and mirrors of patterns and mirrors nest at most {MAX_NESTING} deep")));
            }
            let moved: Vec<Mat> = mats.iter().flat_map(|m| inner_mats.iter().map(move |mi| crate::mat_mul(m, mi))).collect();
            if moved.len() > MAX_INSTANCES || next.saturating_add(moved.len()) > MAX_INSTANCES * MAX_NESTING {
                return Err(DocError::Invalid("too many pattern instances".into()));
            }
            let block = *next;
            *next += moved.len();
            walk(doc, vals, st, &inner, &moved, block, next, depth + 1, out)?;
        }
        Ok(())
    }
    let mut next = mats.len();
    let mut out = Vec::new();
    walk(doc, vals, st, features, mats, 0, &mut next, 0, &mut out)?;
    Ok(out)
}

/// The transforms of a pattern's or mirror's copies in their instance order (instance `k + 1`
/// is the `k`-th): `mats`, then those of the copies made through patterns and mirrors among
/// `features` ([`copy_plan`]). Naming finds a copy's faces through them.
fn copy_instances(doc: &Document, vals: &BTreeMap<String, Value>, st: &ModelState, features: &[FeatureId], mats: &[Mat]) -> Result<Vec<Mat>> {
    let plan = copy_plan(doc, vals, st, features, mats)?;
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
    // The copies are made in timeline order, whatever order the features were picked in: a
    // chamfer picked before the hole it blends would otherwise find no edge to copy.
    let mut kept: Vec<FeatureId> = features.iter().copied().filter(|n| !suppressed.contains(n)).collect();
    let at = |id: &FeatureId| doc.features.iter().position(|x| x.id == id.0).unwrap_or(usize::MAX);
    kept.sort_by_key(at);
    kept.dedup();
    let (mut missed, mut total) = (0usize, 0usize);
    let plan = copy_plan(doc, vals, st, &kept, mats)?;
    for (src, mats, first) in &plan {
        // A fillet or chamfer repeats at the copied edges.
        if let FeatureKind::Fillet { edges, radius, .. } | FeatureKind::Chamfer { edges, distance: radius, .. } = &src.kind {
            let r = val(vals, radius, Kind::Length)?;
            // The edges the feature blended in this evaluation (found by name), falling back to
            // its picked points.
            let found = st.blend_edges.get(&src.id).cloned().unwrap_or_else(|| crate::plain(edges));
            for m in mats {
                let moved_all: Vec<Vec3> = found.iter().map(|p| crate::apply_point(m, *p)).collect();
                // The copies of a blend over several bodies blend each body's own edges.
                let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
                for (i, p) in moved_all.iter().enumerate() {
                    let ti = match body_on_edge(st, *p) {
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
                    let Some(mb) = st.bodies.get(ti).cloned() else { continue };
                    let nb = match &src.kind {
                        FeatureKind::Chamfer { distance2, angle, flip, .. } => chamfer_with(vals, &mb.body, &moved, r, distance2, angle, *flip)?,
                        // A copy is congruent to the original: the radius it was reduced to first.
                        FeatureKind::Fillet { fit, .. } => match st.fitted.get(&src.id) {
                            Some(used) => {
                                let used: Vec<f64> = idx.iter().map(|i| used.get(*i).copied().unwrap_or(r)).collect();
                                kernel::fillet_each(&mb.body, &moved, &used).or_else(|_| fillet_fitted(&mb.body, &moved, r, *fit).map(|x| x.0))?
                            }
                            None => fillet_fitted(&mb.body, &moved, r, *fit)?.0,
                        },
                        _ => kernel::fillet(&mb.body, &moved, r)?,
                    };
                    // A copy blends edges the pattern's own copies made: named after the body so far.
                    let nb = mb.traced().then(nb);
                    if let Some(slot) = st.bodies.get_mut(ti) {
                        *slot = ModelBody::traced_new(mb.name, nb, mb.feature);
                    }
                }
            }
            continue;
        }
        let (op, targets) = feature_op(src);
        // A primitive's or a hole's targets are its participants (#1108): the copies change
        // those bodies only, like the feature itself (#2755).
        let targets = if targets.is_empty() { src.participants.clone() } else { targets };
        // A join that merged its targets left only the first of them (the others now live in
        // it): the copies change the bodies still there (#3378). When none is, the names stay
        // and say which body is missing.
        let present: Vec<String> = targets.iter().filter(|t| st.bodies.iter().any(|b| &b.name == *t)).cloned().collect();
        let targets = if present.is_empty() { targets } else { present };
        let mut copies = Vec::new();
        let mut instances = Vec::with_capacity(mats.len());
        if let FeatureKind::Hole { diameter, depth, hole, to, .. } = &src.kind {
            // A hole is drilled again at each copy of its spot (found on the surface there), so
            // a blind hole's depth counts from the face at the copy, not from the bottom of the
            // original hole under the original spot.
            let (spots, dir) = hole_spots_raw(vals, st, src)?;
            for (k, m) in mats.iter().enumerate() {
                let d = crate::apply_vector(m, dir).normalized().ok_or_else(|| DocError::Invalid("hole direction".into()))?;
                let start = copies.len();
                for p in &spots {
                    let at = hole_on_surface(st, crate::apply_point(m, *p), d, hole_scope(src));
                    let to = to.map(|t| crate::apply_point(m, t.0));
                    let depth = hole_depth_to(st, &at, d, &to, depth)?;
                    // Each copy's faces are named as the hole's, with the instance.
                    copies.extend(
                        hole_tools(vals, st, &at, d, diameter, &depth, hole)?
                            .into_iter()
                            .map(|t| Traced { lineage: t.lineage.map(|l| l.copied(src.id, first + k + 1)), body: t.body }),
                    );
                }
                instances.push(start..copies.len());
            }
        } else if let FeatureKind::Rib { .. } = &src.kind {
            // A rib is built again at each copy of its sketch, against the bodies there (#4593):
            // its walls run until they meet the body, which may not lie the same way there.
            for (k, m) in mats.iter().enumerate() {
                for body in more::rib_tools(vals, st, src, Some(m))? {
                    // Each copy's faces are named as the rib's, with the instance.
                    let t = Traced::unnamed(body);
                    copies.push(Traced { lineage: t.lineage.map(|l| l.copied(src.id, first + k + 1)), body: t.body });
                }
            }
        } else {
            // Each copy's faces are named as the source feature's, with the instance. Its face
            // points follow their named faces, as the feature's own did (#2121): a To Object
            // extrude's copies reach the face where it is now, not where it was picked.
            let moved = with_named_face_points(doc, st, src).map(|(g, _)| g);
            let tools = feature_tools_with_constructor(doc, vals, moved.as_ref().unwrap_or(src), st, &mut None, &mut None)?;
            for (k, m) in mats.iter().enumerate() {
                let start = copies.len();
                for t in &tools {
                    copies.push(t.copied(kernel::transform_matrix(&t.body, *m)?, src.id, first + k + 1));
                }
                instances.push(start..copies.len());
            }
        }
        if op == Operation::Cut {
            for instance in &instances {
                if let Some(tools) = copies.get(instance.clone()) {
                    total += 1;
                    missed += usize::from(copy_misses(st, f, &targets, tools));
                }
            }
        }
        // Copies of a cut through sheet metal are cut-outs of its flat pattern, as the cut is
        // (#6841): each machined into the sheets in turn, then cut from the other targets.
        if op == Operation::Cut && matches!(src.kind, FeatureKind::Extrude { .. }) && !st.sheets.is_empty() && instances.len() == mats.len() {
            let mut handled = Vec::with_capacity(mats.len());
            for (m, instance) in mats.iter().zip(&instances) {
                let bodies: Vec<Body> = copies.get(instance.clone()).unwrap_or_default().iter().map(|t| t.body.clone()).collect();
                handled.push(more::sheet_cut_copy(f, src, m, st, &bodies, &targets, warning)?);
            }
            if handled.iter().any(|h| !h.is_empty()) {
                // Copies the same sheets took are cut from the rest at once.
                let mut groups: Vec<(Vec<String>, Vec<Traced>)> = Vec::new();
                let mut rest = copies.into_iter();
                for (done, instance) in handled.into_iter().zip(&instances) {
                    let tools: Vec<Traced> = rest.by_ref().take(instance.len()).collect();
                    match groups.iter_mut().find(|(d, _)| *d == done) {
                        Some((_, all)) => all.extend(tools),
                        None => groups.push((done, tools)),
                    }
                }
                for (done, tools) in groups {
                    apply_beside_sheet_cut(doc, st, f, tools, op, &targets, &done)?;
                }
                continue;
            }
        }
        // New bodies from a pattern join their source when they touch it (as one feature would).
        if f.body_names.is_empty() {
            apply_traced(doc, st, f, copies, op, &targets)?;
        } else {
            let start = st.bodies.iter().filter(|b| b.feature == f.id).count();
            let mut named = f.clone();
            named.body_names = f.body_names.iter().skip(start).cloned().collect();
            apply_traced(doc, st, &named, copies, op, &targets)?;
        }
    }
    if missed > 0 {
        append_warning(warning, format!("{missed} of {total} cut copies remove no material"));
    }
    Ok(())
}

#[path = "model_cache.rs"]
mod model_cache;
#[path = "eval_more.rs"]
mod more;
pub use model_cache::SavedModel;
#[path = "eval_frame.rs"]
pub(crate) mod frame_member;
#[path = "eval_surface.rs"]
mod surface;

/// Height above `plane` (along its normal) of the object at `p` as seen from `centre`: the face
/// there if it is planar and parallel (its plane), its nearest edge if it is planar and runs
/// along the normal, else where a ray from `centre` along the normal meets the body there, else
/// the point itself.
pub fn object_height(st: &ModelState, plane: &Plane, centre: Vec3, p: Vec3) -> f64 {
    height_at(st, face_at(st, p, Some(plane.normal())), plane, centre, p)
}

/// [`object_height`] of the point `p` on `face` (`(body index, face index)`, see [`object_face`]).
fn height_at(st: &ModelState, face: Option<(usize, usize)>, plane: &Plane, centre: Vec3, p: Vec3) -> f64 {
    let n = plane.normal();
    let h = |q: Vec3| (q - plane.origin).dot(n);
    let Some((bi, fi)) = face else { return h(p) };
    let Some(b) = st.bodies.get(bi) else { return h(p) };
    let tol = (b.body.size() * 1e-3).max(1e-3);
    let Ok(faces) = b.body.faces(tol) else { return h(p) };
    if let Some(f) = faces.iter().find(|f| f.index == fi)
        && f.plane_normal.is_some_and(|fnrm| fnrm.cross(n).len() < 1e-6)
    {
        return h(f.centroid);
    }
    let m = b.mesh();
    // A plane running along the normal (the top of a block ahead of the profile): where the face
    // begins, its nearest edge. Not the pick, which is anywhere along it. A face that starts at
    // the profile's plane or crosses it (a step's side wall) is reached at its end on the pick's side.
    if let Some(f) = faces.iter().find(|f| f.index == fi)
        && f.plane_normal.is_some_and(|fnrm| runs_along(fnrm, n))
    {
        let hs: Vec<f64> =
            m.triangles.iter().zip(&m.tri_face).filter(|(_, x)| **x as usize == fi).filter_map(|(t, _)| m.tri(t)).flatten().map(h).collect();
        let lo = hs.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = hs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if !lo.is_finite() || !hi.is_finite() {
            return h(p);
        }
        // Ahead of the profile: its near edge; behind: its near edge on that side; across the
        // profile's plane: the end on the pick's side.
        let end = if lo > tol || (hi >= -tol && h(p) < 0.0) { lo } else { hi };
        return if end.abs() > tol { end } else { h(p) };
    }
    // A curved or slanted face: where the profile's middle meets it.
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
/// stored `name` finds when the point lies on it (evaluation has moved the point onto its named
/// face), else the nearest face at the point. The name settles a tie: in an assembly a part of
/// another component may rest on the target and touch it at the point.
fn object_face(st: &ModelState, name: Option<&str>, p: Vec3, normal: Option<Vec3>) -> Option<(usize, usize)> {
    if let Some(n) = name.filter(|n| !n.is_empty())
        && let Some(hit) = crate::naming::locate_face(st, n, p)
        && hit.on_point
    {
        return Some((hit.body, hit.face));
    }
    face_at(st, p, normal)
}

/// The body of `face` when that face is not a plane (with `n`: not a plane square to `n`), so To
/// Object must follow its shape.
fn curved_face(st: &ModelState, face: Option<(usize, usize)>, n: Option<Vec3>) -> Option<usize> {
    let (bi, fi) = face?;
    let b = st.bodies.get(bi)?;
    let tol = (b.body.size() * 1e-3).max(1e-3);
    let normal = b.body.faces(tol).ok()?.into_iter().find(|f| f.index == fi).map(|f| f.plane_normal);
    // A plane square to `n` or running along it ends the tool flat (see `height_at`).
    let planar = match n {
        Some(n) => normal.flatten().is_some_and(|fnrm| fnrm.cross(n).len() < 1e-6 || runs_along(fnrm, n)),
        None => normal.is_some_and(|fnrm| fnrm.is_some()),
    };
    (!planar).then_some(bi)
}

/// Does a plane with normal `fnrm` run along the direction `n` (contain it)?
fn runs_along(fnrm: Vec3, n: Vec3) -> bool {
    fnrm.dot(n).abs() < 1e-6
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
            let touches =
                lump.tessellate((lump.size() * 1e-2).max(1e-2)).map(|m| m.positions.iter().any(|q| dist_to_start(*q) < tol)).unwrap_or(false);
            if touches {
                out.push(Traced { body: lump, lineage });
            }
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
                && n1.dot(n2) > 2f64.to_radians().cos()
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

/// `plane` with the sheet face it names (see `sheet::plane_sheet_face`) replaced by that face's current
/// plane in the frame of component `comp`, or an error when the face is gone: a lost
/// attachment is reported, never moved to the nearest face.
pub(crate) fn sheet_face_plane_in_frame(doc: &Document, st: &ModelState, comp: u64, plane: &PlaneRef) -> Result<PlaneRef> {
    Ok(match plane {
        PlaneRef::Face { name: Some(name), anchor: None, .. } if crate::sheet::plane_sheet_face(plane).is_some() => {
            let lost = || DocError::Invalid(format!("lost face attachment: the sheet face `{name}` no longer exists; reattach the sketch to a face"));
            let (body, face) = crate::sheet::sheet_face_plane(st, name).ok_or_else(lost)?;
            if doc.occurrences.is_empty() && doc.body_offsets.is_empty() {
                return Ok(PlaneRef::Custom { plane: face });
            }
            // The body's own coordinates into the component's frame, as `state_in_frame` places its bodies.
            let source = st.body(&body).ok_or_else(lost)?;
            let inverse = crate::mat_inverse(&doc.component_transform(comp)).ok_or_else(lost)?;
            let m = crate::mat_mul(&inverse, &doc.component_transform(doc.model_body_component(source)));
            let m = doc.model_body_offset(source).map(|offset| crate::mat_mul(&m, offset)).unwrap_or(m);
            let placed = Plane::new(crate::apply_point(&m, face.origin), crate::apply_vector(&m, face.x), crate::apply_vector(&m, face.y));
            PlaneRef::Custom { plane: placed.ok_or_else(lost)? }
        }
        PlaneRef::Offset { base, distance } => {
            PlaneRef::Offset { base: Box::new(sheet_face_plane_in_frame(doc, st, comp, base)?), distance: distance.clone() }
        }
        PlaneRef::AtAngle { base, axis_origin, axis_dir, angle, line } => PlaneRef::AtAngle {
            base: Box::new(sheet_face_plane_in_frame(doc, st, comp, base)?),
            axis_origin: *axis_origin,
            axis_dir: *axis_dir,
            angle: angle.clone(),
            line: line.clone(),
        },
        PlaneRef::Flipped { base, about_y } => {
            PlaneRef::Flipped { base: Box::new(sheet_face_plane_in_frame(doc, st, comp, base)?), about_y: *about_y }
        }
        PlaneRef::ThroughPoint { base, point } => {
            PlaneRef::ThroughPoint { base: Box::new(sheet_face_plane_in_frame(doc, st, comp, base)?), point: point.clone() }
        }
        other => other.clone(),
    })
}

/// The plane of the body face a sketch sits on, found again: among planar faces with the
/// picked plane's normal, the one nearest `at`. The picked frame moves along its normal onto it.
fn face_plane(st: &ModelState, picked: &Plane, at: Vec3, name: Option<&str>) -> (Plane, Option<String>) {
    let n = picked.normal();
    // By name first: the face (or a piece of it) with that name, facing the same way.
    if let Some(name) = name {
        // (distance from `at`, plane offset along n, what to say about how it was found)
        let mut found: Option<(f64, f64, Option<String>)> = None;
        for b in st.bodies.iter().filter(|b| !b.body.is_mesh()) {
            let named = crate::naming::faces_with_stem(b, name);
            if named.is_empty() {
                continue;
            }
            let tol = (b.body.size() * 1e-3).max(1e-3);
            let Ok(faces) = b.body.faces(tol) else { continue };
            for f in faces {
                let hit = named.contains(&f.index);
                if let (true, Some(fnrm)) = (hit, f.plane_normal)
                    && fnrm.dot(n) > 1.0 - 1e-6
                {
                    let d = f.centroid.dist(at);
                    if found.as_ref().is_none_or(|x| d < x.0) {
                        // A name saved before format 14 is found as it was then, without a note.
                        let note = if crate::naming::legacy_name(name).is_some() {
                            None
                        } else if !crate::naming::face_has_name(b, f.index, name) {
                            Some(format!("face `{name}` was split; using the piece nearest the picked point"))
                        } else if !crate::naming::face_name_exact(b, f.index) && (f.centroid - at).dot(n).abs() > tol {
                            // Named from geometry and moved off the picked plane: geometry alone decided.
                            Some(format!("face `{name}` was found by geometry, not exact history; check it"))
                        } else {
                            None
                        };
                        found = Some((d, f.centroid.dot(n), note));
                    }
                }
            }
        }
        if let Some((_, off, note)) = found {
            return (picked.offset(off - picked.origin.dot(n)), note);
        }
        let (pl, w) = face_plane(st, picked, at, None);
        let note = format!(
            "the face `{name}` the sketch was on no longer exists; {}",
            if w.is_some() { "the sketch stays where it was" } else { "using the face at the picked point" }
        );
        return (pl, Some(note));
    }
    let mut best: Option<(f64, f64)> = None; // (distance from `at`, plane offset along n)
    for b in &st.bodies {
        let tol = (b.body.size() * 1e-3).max(1e-3);
        let (Ok(faces), Ok(mesh)) = (b.body.faces(tol), b.body.tessellate(tol)) else { continue };
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
                best = Some((d, f.centroid.dot(n)));
            }
        }
    }
    match best {
        Some((_, off)) => {
            let shift = off - picked.origin.dot(n);
            (picked.offset(shift), None)
        }
        None => (*picked, Some("the face the sketch was on is gone; the sketch stays where it was".into())),
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
        Some(s) => kernel::chamfer_sides(b, edges, d, s, flip)?,
        None => kernel::chamfer(b, edges, d)?,
    })
}

fn eval_feature(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState, warning: &mut Option<String>) -> Result<()> {
    // Its tools meet bodies in other placements where those are shown (#3383).
    let _frames = ToolFrames::install(doc, f, st);
    match &f.kind {
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
        | FeatureKind::SheetUnfold { .. }
        | FeatureKind::SheetConvert { .. } => more::sheet_eval(doc, vals, f, st, warning),
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
        | FeatureKind::Remove { .. } => more::eval(doc, vals, f, st),
        FeatureKind::Sketch { plane, sketch } => {
            let anchor = crate::face_anchor::plane_face_anchor(plane);
            let fst = match anchor {
                Some(anchor) => crate::state_in_face_anchor_frame(doc, st, anchor, f.component)?,
                // A sketch that reads no geometry (a fixed plane, no projections) needs no other
                // component's bodies moved into its frame: placing every body of a large
                // assembly again is what made each sketch edit as slow as the design is big.
                None if !plane_reads_geometry(plane) && sketch.links.is_empty() => std::borrow::Cow::Borrowed(st),
                None => state_in_frame(doc, st, f.component),
            };
            let (plane, face_warning) = match (anchor, plane) {
                (Some(_), plane) => doc.resolve_attached_plane_noted(vals, plane, f.component, st, &fst)?,
                // A flat face of a sheet: the panel's exact plane, from its flat region and fold.
                (None, plane) if crate::sheet::plane_sheet_face(plane).is_some() => {
                    (doc.resolve_plane_in(vals, &sheet_face_plane_in_frame(doc, st, f.component, plane)?, 0, Some(&fst))?, None)
                }
                (None, PlaneRef::Face { plane: picked, at, name, anchor: None }) => face_plane(&fst, picked, *at, name.as_deref()),
                (None, other) => (doc.resolve_plane_in(vals, other, 0, Some(&fst))?, None),
            };
            let mut sk = sketch.clone();
            // A text's outlines are rebuilt from its numbers when the links are refreshed: the
            // ones its expressions give come first.
            doc.apply_text_values(vals, &mut sk)?;
            let mut warns = match anchor {
                Some(anchor) => crate::project::refresh_face_anchor_links(doc, vals, &fst, &plane, &mut sk, anchor),
                None => crate::project::refresh_links(doc, vals, &fst, &plane, &mut sk),
            };
            drop(fst);
            sk.refresh_offsets()?;
            if let Some(w) = face_warning {
                warns.insert(0, w);
            }
            // The stored sketch is solved for its stored values, with its links where they
            // were stored: a link that moved must not change the side a dimension from it is on.
            let stored = sk.driving_values();
            let report = doc.solve_sketch(vals, &mut sk, &stored, Some(sketch))?;
            if !report.keeps_orientation() {
                warns.insert(0, report.failure(|f| format!("the sketch constraints conflict ({f})")));
            } else {
                // The solve may have moved the curve a text runs along.
                warns.extend(crate::project::relay_path_texts(&mut sk));
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
            let ends = solidcraft_sketch::loose_ends(&sk);
            st.sketches.retain(|s| s.feature != f.id);
            st.sketches.push(Arc::new(SolvedSketch { feature: f.id, name: f.name.clone(), plane, sketch: sk, report, profiles, ends }));
            Ok(())
        }
        FeatureKind::Extrude { operation, targets, .. }
        | FeatureKind::Pipe { operation, targets, .. }
        | FeatureKind::Revolve { operation, targets, .. }
        | FeatureKind::Loft { operation, targets, .. }
        | FeatureKind::Coil { operation, targets, .. }
        | FeatureKind::Sweep { operation, targets, .. } => {
            let mut constructor = None;
            let mut to_body = None;
            let tools = feature_tools_with_constructor(doc, vals, f, st, &mut constructor, &mut to_body)?;
            // Cuts through sheet metal become cut-outs of its flat pattern.
            if matches!(operation, Operation::Cut | Operation::Intersect) && matches!(f.kind, FeatureKind::Extrude { .. }) && !st.sheets.is_empty() {
                let bodies: Vec<Body> = tools.iter().map(|t| t.body.clone()).collect();
                let handled = more::sheet_cut(f, st, &bodies, &crate::plain(targets), constructor.as_ref(), warning)?;
                if !handled.is_empty() {
                    return apply_beside_sheet_cut(doc, st, f, tools, *operation, &crate::plain(targets), &handled);
                }
            }
            // A Join extrude to an object joins that object too (#6424): the tool and the body
            // it runs to are one body, not only the body the profile was sketched on.
            let extra: Vec<String> = if *operation == Operation::Join { to_body.into_iter().collect() } else { Vec::new() };
            apply_traced_with(doc, st, f, tools, *operation, &crate::plain(targets), &extra)
        }
        FeatureKind::Hole { thread, thread_depth, diameter, handedness, .. } => {
            let thread_depth = thread_depth.as_ref().map(|d| val(vals, d, Kind::Length)).transpose()?;
            if thread_depth.is_some() && thread.is_none() {
                return Err(DocError::Invalid("thread depth requires a tapped hole".into()));
            }
            if thread_depth.is_some_and(|d| !d.is_finite() || d <= 0.0) {
                return Err(DocError::Invalid("thread depth must be finite and positive".into()));
            }
            let (spots, dir) = hole_spots(vals, st, f)?;
            let tools = hole_feature_tools(vals, st, f)?;
            let handled = sheet_hole_cut(st, f, &tools, warning)?;
            let tools: Vec<Traced> = tools.into_iter().map(|(tool, _)| tool).collect();
            if handled.is_empty() {
                apply_traced(doc, st, f, tools, Operation::Cut, &[])?;
            } else {
                apply_beside_sheet_cut(doc, st, f, tools, Operation::Cut, &[], &handled)?;
            }
            if let Some(d) = thread {
                // Find the bore beyond any entry recess, including shallow and through holes.
                let r = val(vals, diameter, Kind::Length)? / 2.0;
                let parallel_pipe = crate::pipe_thread_catalog::parse_pipe_thread(d).is_some();
                for p in spots {
                    let wall = if parallel_pipe {
                        parallel_tapped_wall(st, p, dir, r).ok_or_else(|| DocError::Invalid("no cylindrical pipe-thread wall remains".into()))?
                    } else {
                        parallel_tapped_wall(st, p, dir, r).unwrap_or(p + dir.any_perp() * r + dir * (r * 0.5))
                    };
                    let mut t = thread_on_impl(st, f.id, wall, Some(d), None, true, false, *handedness)?;
                    if let Some(depth) = thread_depth {
                        let available = t.end - t.start;
                        if depth > available {
                            return Err(DocError::Invalid(format!(
                                "thread depth {} mm exceeds the cylindrical wall's useful length of {} mm; reduce the thread depth or use full thread depth",
                                show_mm(depth),
                                show_mm(available)
                            )));
                        }
                        // Start at the entry in the drilling direction, independently of the
                        // wall probe's position and the cylinder surface's axis orientation.
                        if dir.dot(t.axis) > 0.0 {
                            t.end = t.start + depth;
                        } else {
                            t.start = t.end - depth;
                        }
                    }
                    st.threads.push(t);
                }
            }
            Ok(())
        }
        FeatureKind::Thread { face, designation, length, modeled, handedness } => {
            let l = match length {
                Some(e) => Some(val(vals, e, Kind::Length)?),
                None => None,
            };
            let t = thread_on(st, f.id, face.0, designation.as_deref(), l, *modeled, *handedness)?;
            if !*modeled {
                st.threads.push(t);
                return Ok(());
            }
            let i = st.bodies.iter().position(|b| b.name == t.body).ok_or_else(|| DocError::Invalid("threaded body".into()))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let cyl = kernel::cylinder_face_at(&mb.body, face.0).ok_or_else(|| DocError::Invalid("no cylindrical face there to thread".into()))?;
            let tol = t.pitch * 1e-3;
            // A shaft thicker than the thread is turned down to its major diameter first.
            let input = mb.traced();
            let (input, radius) = match thread_turn(&t, &cyl, &mb)? {
                Some(ring) => {
                    let turned = kernel::boolean(&input.body, &ring.body, BoolOp::Cut)?
                        .ok_or_else(|| DocError::Invalid("turning the shaft down removed everything".into()))?;
                    (Traced::step(turned, &[&input, &ring]), t.major_diameter / 2.0)
                }
                None => (input, cyl.radius),
            };
            let groove = thread_groove(&t, radius, ((t.start - cyl.start).abs() < tol, (t.end - cyl.end).abs() < tol))?;
            // The B-rep boolean can't take helical walls in reasonable time: the body is cut
            // faceted (curved faces within a two-hundredth of the pitch).
            let nb = kernel::faceted_boolean(&input.body, &groove, BoolOp::Cut, t.pitch * 0.005)?
                .ok_or_else(|| DocError::Invalid("the thread removed everything".into()))?;
            // The groove's faces are its triangles, with no role to name them by.
            let groove = crate::lineage::Lineage::unnamed_faces(groove.triangles.len());
            let lineage = input.lineage.and_then(|l| crate::lineage::Lineage::step(&nb, &[&l, &groove]));
            let nb = Traced { body: nb, lineage };
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::traced_new(mb.name, nb, mb.feature);
            }
            Ok(())
        }
        FeatureKind::Shell { faces, thickness, body, direction, tangent_chain } => {
            let t = val(vals, thickness, Kind::Length)?;
            let i = body_at(st, body, &crate::plain(faces))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let picks = crate::plain(faces);
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
                    let cavity = Traced::step(kernel::offset_faces(&cavity.body, &open, 2.0 * t + 1.0)?, &[&cavity]);
                    let cut = kernel::boolean(&outer.body, &cavity.body, BoolOp::Cut)?
                        .ok_or_else(|| DocError::Invalid("the shell removed everything".into()))?;
                    (cut, vec![outer, cavity])
                }
                o => return Err(DocError::Invalid(format!("unknown shell direction `{o}` (inside, outside or both)"))),
            };
            // Certifying may join faces; their history stays over the same operands.
            let nb = Traced::step(nb.with_shell_certificate(f.id)?, &operands.iter().collect::<Vec<_>>());
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::traced_new(mb.name, nb, mb.feature);
            }
            Ok(())
        }
        FeatureKind::Draft { faces, angle, neutral, pull, body } => {
            let a = val(vals, angle, Kind::Angle)?;
            let pl = doc.resolve_plane_in(vals, neutral, 0, Some(st))?;
            let i = body_at(st, body, &crate::plain(faces))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let nb = Traced::step(kernel::draft(&mb.body, &crate::plain(faces), &pl, pull.0, a)?, &[&mb.traced()]);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::traced_new(mb.name, nb, mb.feature);
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
                // On a flat face of a sheet: that panel's plane as it is now, as a sketch on the
                // face finds it (the stored plane is only where the face was picked).
                None if crate::sheet::plane_sheet_face(plane).is_some() => {
                    doc.resolve_plane_in(vals, &sheet_face_plane_in_frame(doc, st, f.component, plane)?, 0, Some(st))?
                }
                // A plane of numbers alone reads no body: placing them all is the design's size.
                None if plane_is_fixed(plane) => doc.resolve_plane_in(vals, plane, 0, Some(st))?,
                // Picks on bodies of other (moved) components are where those bodies are shown.
                None => doc.resolve_plane_in(vals, plane, 0, Some(&construction_state(doc, st, f.component)))?,
            };
            st.construct.push(crate::construct::Construct { feature: f.id, name: f.name.clone(), geom: crate::construct::ConstructGeom::Plane(pl) });
            Ok(())
        }
        FeatureKind::ConstructionAxis { def } => {
            let (origin, dir) =
                crate::construct::resolve_axis(&crate::construct::Ctx { doc, vals, st: &construction_state(doc, st, f.component), depth: 0 }, def)?;
            st.construct.push(crate::construct::Construct {
                feature: f.id,
                name: f.name.clone(),
                geom: crate::construct::ConstructGeom::Axis { origin, dir },
            });
            Ok(())
        }
        FeatureKind::ConstructionPoint { def } => {
            let p =
                crate::construct::resolve_point(&crate::construct::Ctx { doc, vals, st: &construction_state(doc, st, f.component), depth: 0 }, def)?;
            st.construct.push(crate::construct::Construct { feature: f.id, name: f.name.clone(), geom: crate::construct::ConstructGeom::Point(p) });
            Ok(())
        }
        FeatureKind::BoundaryFill { tools, cells, operation, remove_tools } => {
            if tools.is_empty() || tools.len() > 64 || cells.is_empty() || cells.len() > 1000 {
                return Err(DocError::Invalid("Boundary Fill needs 1…64 tool bodies and 1…1000 cells".into()));
            }
            let tb: Vec<ModelBody> =
                tools.iter().map(|t| st.body(t).cloned().ok_or_else(|| DocError::Unknown(format!("body `{t}`")))).collect::<Result<_>>()?;
            let mut made = Vec::new();
            for p in cells {
                // The cell: inside the tools that hold the point, outside the others.
                let inside: Vec<bool> = tb.iter().map(|b| b.mesh().contains(p.0)).collect();
                let mut acc: Option<Traced> = None;
                for (b, _) in tb.iter().zip(&inside).filter(|(_, i)| **i) {
                    acc = match acc {
                        None => Some(b.traced()),
                        Some(a) => kernel::boolean(&a.body, &b.body, BoolOp::Intersect)?.map(|r| Traced::step(r, &[&a, &b.traced()])),
                    };
                }
                let Some(mut cell) = acc else {
                    return Err(DocError::Invalid(format!("the point {p:?} is outside every tool: no enclosed cell there")));
                };
                for (b, _) in tb.iter().zip(&inside).filter(|(_, i)| !**i) {
                    let cut = kernel::boolean(&cell.body, &b.body, BoolOp::Cut)?.ok_or_else(|| DocError::Invalid("a cell came out empty".into()))?;
                    cell = Traced::step(cut, &[&cell, &b.traced()]);
                }
                made.push(cell);
            }
            if *remove_tools {
                st.bodies.retain(|b| !tools.iter().any(|t| *t == b.name));
            }
            apply_traced(doc, st, f, made, *operation, &[])
        }
        FeatureKind::SplitFace { faces, body, plane, tool, sketch } => {
            let i = body_at(st, body, &crate::plain(faces))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let mut picked: Vec<usize> = Vec::new();
            for p in faces {
                let fi = crate::appearance::face_index_at(&mb, p.0)
                    .ok_or_else(|| DocError::Invalid(format!("Split Face: no face of {} at {p:?}", mb.name)))?;
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
                    // Each curve's exact segments (lines and arcs split by their own plane and cylinder).
                    let curves: Vec<Vec<solidcraft_geom::Seg2>> = (0..ss.sketch.curves.len())
                        .filter(|ci| ss.sketch.curves.get(*ci).is_some_and(|c| !c.construction))
                        .map(|ci| ss.sketch.segs(ci))
                        .filter(|segs| !segs.is_empty())
                        .collect();
                    if curves.is_empty() {
                        return Err(DocError::Invalid(format!("sketch `{}` has no curves to split with", ss.name)));
                    }
                    kernel::split_faces(&mb.body, &picked, &kernel::SplitTool::Curves { plane: ss.plane, curves, along: None })?
                }
                (None, None) => {
                    let pl = doc.resolve_plane_in(vals, plane, 0, Some(st))?;
                    kernel::split_faces(&mb.body, &picked, &kernel::SplitTool::Plane(pl))?
                }
            };
            let nb = Traced::step(nb, &[&mb.traced()]);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::traced_new(mb.name, nb, mb.feature);
            }
            Ok(())
        }
        FeatureKind::Split { body, plane, tool, sketch, line } => {
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
                    // The pieces keep the planar roles of the faces they share with the body (#565).
                    kernel::split_body(&mb.body, &kernel::SplitTool::Face { body: &tb.body, face })?
                        .into_iter()
                        .map(|part| Ok(Traced::step(part.with_preserved_planar_sources(&mb.body)?, &[&input])))
                        .collect::<Result<Vec<_>>>()?
                }
                None => {
                    let pl = doc.resolve_plane_in(vals, plane, 0, Some(st))?;
                    split_by_plane_traced(&input, &pl)?
                }
            };
            if parts.len() < 2 {
                return Err(DocError::Invalid("the tool does not split the body".into()));
            }
            let mut it = parts.into_iter();
            if let (Some(first), Some(slot)) = (it.next(), st.bodies.get_mut(i)) {
                *slot = ModelBody::traced_new(mb.name.clone(), first, mb.feature);
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
                st.bodies.push(ModelBody::traced_new(name, b, f.id));
            }
            Ok(())
        }
        FeatureKind::Pattern { features, pattern, bodies } => {
            let mats = pattern_transforms(doc, vals, st, f.component, pattern)?;
            if bodies.is_empty() {
                return replay(doc, vals, f, st, features, &mats, warning);
            }
            // Bodies: each copy is a new body named after its source.
            let here = doc.component_transform(f.component);
            let mut output = 0;
            for n in bodies {
                let mb = st.body(n).cloned().ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                // A body of another component is held in that component's frame, the pattern's
                // transforms in this feature's: each copy is moved as the pattern moves it in the
                // world and kept beside its source, in its component (#4763). Taken as they were,
                // the copies landed where the source sits before its component is placed.
                let owner = (mb.component != f.component).then(|| {
                    let frame = doc.component_transform(mb.component);
                    let singular = || DocError::Invalid(format!("the component holding `{n}` has a singular placement"));
                    Ok::<_, DocError>((frame, crate::mat_inverse(&frame).ok_or_else(singular)?, crate::mat_inverse(&here).ok_or_else(singular)?))
                });
                let owner = owner.transpose()?;
                for (k, m) in mats.iter().enumerate() {
                    let local = match &owner {
                        Some((frame, frame_inv, here_inv)) => {
                            crate::mat_mul(&crate::mat_mul(frame_inv, &here), &crate::mat_mul(&crate::mat_mul(m, here_inv), frame))
                        }
                        None => *m,
                    };
                    let copy = moved_copy(&mb, kernel::transform_matrix(&mb.body, local)?, k + 1);
                    let name = pattern_copy_name(st, f, output, &mb.name);
                    copy_sheet(st, &mb.name, &name, &local);
                    let mut body = ModelBody::traced_new(name, copy, f.id);
                    if owner.is_some() {
                        // Kept in that component as a moved body is (and through later features).
                        body.component = mb.component;
                        body.transferred = true;
                    }
                    st.bodies.push(body);
                    output += 1;
                }
            }
            Ok(())
        }
        FeatureKind::Mirror { features, plane, bodies, combine, placements } => {
            let pl = doc.resolve_plane_in(vals, plane, 0, Some(st))?;
            if bodies.is_empty() {
                return replay(doc, vals, f, st, features, &[mirror_matrix(&pl)], warning);
            }
            let m = mirror_matrix(&pl);
            let mut output = 0;
            for (k, n) in bodies.iter().enumerate() {
                let i = st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                let Some(mb) = st.bodies.get(i).cloned() else { continue };
                // A body picked where a placement shows it is mirrored there: the copy goes beside
                // it into the component that placement shows, a joined body stays in its own frame.
                let (component, copy, own) = match mirrored_placement(doc, &mb, placements.get(k).map(Vec::as_slice))? {
                    Some((component, frame, source)) => {
                        let singular = || DocError::Invalid(format!("the placement `{n}` was mirrored at is singular"));
                        let inverse = |m: &Mat| crate::mat_inverse(m).ok_or_else(singular);
                        // The plane is in this feature's component: the body mirrored in the world.
                        let here = doc.component_transform(f.component);
                        let world = crate::mat_mul(&crate::mat_mul(&here, &m), &crate::mat_mul(&inverse(&here)?, &source));
                        (Some(component), crate::mat_mul(&inverse(&frame)?, &world), crate::mat_mul(&inverse(&source)?, &world))
                    }
                    None => (None, m, m),
                };
                let mirrored = moved_copy(&mb, kernel::transform_matrix(&mb.body, if *combine { own } else { copy })?, 1);
                if *combine {
                    let joined = kernel::boolean(&mb.body, &mirrored.body, BoolOp::Union)?
                        .ok_or_else(|| DocError::Invalid("the mirror join produced nothing".into()))?;
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
            let on = blend_bodies(st, f, body, &crate::plain(edges))?;
            let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
            for (i, ti) in on.iter().enumerate() {
                match groups.iter_mut().find(|(b, _)| b == ti) {
                    Some((_, idx)) => idx.push(i),
                    None => groups.push((*ti, vec![i])),
                }
            }
            let (mut found, mut notes, mut fitted, mut tapered_all) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            let mut reduced = false;
            let mut blended = Vec::new();
            for (ti, idx) in &groups {
                let Some(mb) = st.bodies.get(*ti) else { return Err(DocError::Invalid("body".into())) };
                let mut w = None;
                let edges = blend_edges_on(mb, f, edges, idx, &mut w)?;
                if let Some(w) = w {
                    notes.push(w);
                }
                let mut note = None;
                let (nb, used, tapered) = blend_body(vals, f, mb, &edges, r, &mut note)?;
                if let Some(n) = note {
                    notes.push(n);
                }
                reduced |= used.is_some();
                fitted.extend(used.unwrap_or_else(|| vec![r; edges.len()]));
                tapered_all.extend(tapered);
                found.extend(edges);
                blended.push((*ti, nb));
            }
            if !notes.is_empty() {
                *warning = Some(notes.join("; "));
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
        FeatureKind::Box { corner, length, width, height, operation } => {
            let s = Vec3::new(val(vals, length, Kind::Length)?, val(vals, width, Kind::Length)?, val(vals, height, Kind::Length)?);
            let corner = point(vals, corner)?;
            let b = kernel::box_solid(corner, corner + s)?;
            let b = if matches!(operation, Operation::NewBody | Operation::Join) { b.with_box_certificate(f.id)? } else { b };
            let handled = more::sheet_primitive_cut(f, st, &b, more::PrimitivePrism::Box { lo: corner, hi: corner + s }, *operation, false, warning)?;
            let tools = vec![crate::lineage::box_tool(b)];
            if !handled.is_empty() {
                return apply_beside_sheet_cut(doc, st, f, tools, *operation, &[], &handled);
            }
            apply_traced(doc, st, f, tools, *operation, &[])
        }
        FeatureKind::Cylinder { base, axis, radius, height, operation, .. } => {
            let base = cylinder_base(vals, st, f, base, axis.0)?;
            let (radius, height) = (val(vals, radius, Kind::Length)?, val(vals, height, Kind::Length)?);
            let b = kernel::cylinder(base, axis.0, radius, height)?;
            let b = if matches!(operation, Operation::NewBody | Operation::Join) { b.with_cylinder_certificate(f.id)? } else { b };
            let prism = more::PrimitivePrism::Cylinder { base, axis: axis.0, radius, height };
            let handled = more::sheet_primitive_cut(f, st, &b, prism, *operation, false, warning)?;
            let tools = vec![crate::lineage::cylinder_tool(b)];
            if !handled.is_empty() {
                return apply_beside_sheet_cut(doc, st, f, tools, *operation, &[], &handled);
            }
            apply_traced(doc, st, f, tools, *operation, &[])
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
            let b = more::cone(point(vals, base)?, axis.0, radius, top_radius, height)?;
            apply_op(doc, st, f, vec![b], *operation, &[])
        }
        FeatureKind::Combine { target, tools, operation, keep_tools } => {
            if tools.iter().any(|t| t == target) {
                return Err(DocError::Invalid("a body cannot be combined with itself".into()));
            }
            // Each body is kept in its own component's frame: a tool placed differently from the
            // target is moved into the target's frame, so it is combined where it is shown (#3383).
            // A tool placed exactly as the target is left as it is.
            let target_world = st.body(target).map(|b| body_world(doc, b));
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
                    *mb = ModelBody::traced_new(mb.name.clone(), nb, mb.feature);
                }
            }
            Ok(())
        }
        FeatureKind::OffsetFace { faces, distance, body } => {
            let d = val(vals, distance, Kind::Length)?;
            let i = body_at(st, body, &crate::plain(faces))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let nb = Traced::step(kernel::offset_faces(&mb.body, &crate::plain(faces), d)?, &[&mb.traced()]);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::traced_new(mb.name, nb, mb.feature);
            }
            Ok(())
        }
        FeatureKind::DeleteFace { faces, body, heal } => {
            let i = body_at(st, body, &crate::plain(faces))?;
            let Some(mb) = st.bodies.get(i).cloned() else { return Err(DocError::Invalid("body".into())) };
            let mut picked: Vec<usize> = Vec::new();
            for p in faces {
                let fi = crate::appearance::face_index_at(&mb, p.0)
                    .ok_or_else(|| DocError::Invalid(format!("Delete Face: no face of {} at {p:?}", mb.name)))?;
                if !picked.contains(&fi) {
                    picked.push(fi);
                }
            }
            let all = kernel::with_same_surface(&mb.body, &picked);
            let left = if *heal { kernel::delete_faces(&mb.body, &all)? } else { kernel::remove_faces(&mb.body, &all)? };
            let nb = Traced::step(left, &[&mb.traced()]);
            if let Some(slot) = st.bodies.get_mut(i) {
                *slot = ModelBody::traced_new(mb.name, nb, mb.feature);
            }
            Ok(())
        }
        FeatureKind::FrameMember { .. } => frame_member::evaluate(doc, vals, f, st),
        FeatureKind::BoundingSolid { bodies, margin } => {
            let m = val(vals, margin, Kind::Length)?;
            let mut bb = solidcraft_geom::Aabb3::EMPTY;
            for b in st.bodies.iter().filter(|b| bodies.is_empty() || bodies.iter().any(|t| *t == b.name)) {
                bb = bb.union(&b.mesh().bounds());
            }
            if bb.is_empty() {
                return Err(DocError::Invalid("no bodies to bound".into()));
            }
            let pad = Vec3::new(m, m, m);
            let b = crate::lineage::numbered_tool(kernel::box_solid(bb.min - pad, bb.max + pad)?);
            let name = unique_body_name(st, f.body_names.first().map(String::as_str).unwrap_or("Bounding"));
            st.bodies.push(ModelBody::traced_new(name, b, f.id));
            Ok(())
        }
        FeatureKind::Move { bodies, translate, rotate_axis, angle, copy } => {
            let t =
                Vec3::new(val(vals, &translate[0], Kind::Length)?, val(vals, &translate[1], Kind::Length)?, val(vals, &translate[2], Kind::Length)?);
            let ang = match angle {
                Some(a) => val(vals, a, Kind::Angle)?,
                None => 0.0,
            };
            let axis = rotate_axis.map(Vec3::from).unwrap_or(Vec3::Z);
            // With `copy` set, the source body is kept in place and the transform lands on a new
            // body pushed beside it instead of replacing it.
            let mut output = 0;
            for n in bodies {
                let i = st.bodies.iter().position(|b| &b.name == n).ok_or_else(|| DocError::Unknown(format!("body `{n}`")))?;
                let Some(mb) = st.bodies.get(i).cloned() else { continue };
                let nb = kernel::transform(&mb.body, t, Vec3::ZERO, axis, ang)?;
                if *copy {
                    let traced = moved_copy(&mb, nb, 1);
                    let name = copy_body_name(st, f, output, &mb.name);
                    copy_sheet(st, &mb.name, &name, &crate::rigid(t, Vec3::ZERO, axis, ang));
                    st.bodies.push(ModelBody::traced_new(name, traced, f.id));
                    output += 1;
                } else if let Some(slot) = st.bodies.get_mut(i) {
                    *slot = ModelBody::traced_new(mb.name.clone(), Traced::step(nb, &[&mb.traced()]), mb.feature);
                }
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
        FeatureKind::MeshImport { meshes, .. } => {
            for (k, (name, b)) in mesh_bodies(meshes)?.into_iter().enumerate() {
                let name = copy_body_name(st, f, k, &name);
                st.bodies.push(ModelBody::new(name, b, f.id));
            }
            Ok(())
        }
        FeatureKind::Import { step, skip, .. } => {
            let imp = kernel::step_import_shared(step)?;
            let digest = crate::config::payload_digest(step);
            let names = import_body_names(st, &f.body_names, &imp, skip);
            for (k, (b, name)) in imp.bodies.iter().zip(names).enumerate() {
                let Some(name) = name else { continue };
                let carried = f.link_faces.get(k).map_or(&[][..], Vec::as_slice);
                let b = crate::lineage::linked_tool(b.body.clone().with_import_certificate(&digest, f.id)?, f.id, carried);
                st.bodies.push(ModelBody::traced_new(name, b, f.id));
            }
            if !imp.warnings.is_empty() {
                let n = imp.warnings.len();
                let shown: Vec<&str> = imp.warnings.iter().take(3).map(String::as_str).collect();
                let more = if n > 3 { format!(" (+{} more)", n - 3) } else { String::new() };
                *warning = Some(format!("{}{more}", shown.join("; ")));
            }
            Ok(())
        }
    }
}

/// `copy`, a moved copy of `source` (instance `instance` of a pattern or mirror): its faces are
/// named after the source's.
fn moved_copy(source: &ModelBody, copy: Body, instance: usize) -> Traced {
    Traced::step(copy, &[&source.before_as(crate::lineage::Operand::Copy { body: source.name.clone(), instance })])
}

/// Mesh bodies of a MeshImport feature.
fn mesh_bodies(meshes: &[crate::MeshData]) -> Result<Vec<(String, Body)>> {
    let mut out = Vec::with_capacity(meshes.len());
    for m in meshes {
        let positions: Vec<Vec3> = m.positions.as_chunks::<3>().0.iter().map(|c| Vec3::new(c[0], c[1], c[2])).collect();
        let triangles: Vec<[u32; 3]> = m.triangles.as_chunks::<3>().0.to_vec();
        let b = kernel::mesh_body(&positions, &triangles).map_err(|e| DocError::Invalid(format!("mesh `{}`: {e}", m.name)))?;
        out.push((m.name.clone(), b.with_color(m.color)));
    }
    Ok(out)
}

/// The names an Import feature gives the file's bodies, added after the bodies of `st`, in the
/// file's order: `None` for a body it skips (a later instance of a repeated part). `body_names`
/// are the feature's stored names (by index), else the file's own.
pub fn import_body_names(st: &ModelState, body_names: &[String], imp: &kernel::StepImport, skip: &[u64]) -> Vec<Option<String>> {
    let skip: std::collections::HashSet<u64> = skip.iter().copied().collect();
    // Files from board and assembly tools give thousands of bodies the same name: name them
    // from a set, not by searching the bodies for every candidate (cubic).
    let mut names = BodyNames::new(st);
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
fn construction_state<'a>(doc: &Document, st: &'a ModelState, comp: u64) -> std::borrow::Cow<'a, ModelState> {
    if comp == 0 && !doc.occurrences.is_empty() {
        return std::borrow::Cow::Owned(world_state(doc, st));
    }
    state_in_frame(doc, st, comp)
}

/// Whether `plane` is given by the origin planes and numbers alone.
fn plane_is_fixed(plane: &PlaneRef) -> bool {
    match plane {
        PlaneRef::Origin { .. } | PlaneRef::Custom { .. } | PlaneRef::Placed { .. } => true,
        PlaneRef::AtAngle { line: Some(_), .. } => false,
        PlaneRef::Offset { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => plane_is_fixed(base),
        _ => false,
    }
}

/// Whether resolving `plane` reads the model's bodies or sketches (a face, edges, points),
/// rather than only the origin planes, numbers and construction planes.
fn plane_reads_geometry(plane: &PlaneRef) -> bool {
    match plane {
        PlaneRef::Origin { .. } | PlaneRef::Custom { .. } | PlaneRef::Placed { .. } | PlaneRef::Construction { .. } => false,
        PlaneRef::AtAngle { line: Some(_), .. } => true,
        PlaneRef::Offset { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => plane_reads_geometry(base),
        PlaneRef::ThroughPoint { .. }
        | PlaneRef::Midplane { .. }
        | PlaneRef::TwoEdges { .. }
        | PlaneRef::AxisPoint { .. }
        | PlaneRef::ThreePoints { .. }
        | PlaneRef::Tangent { .. }
        | PlaneRef::AlongPath { .. }
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
        _ => false,
    }
}

/// The frames of [`TOOL_FRAMES`] for the feature being evaluated; what was there before is put
/// back when it is dropped.
struct ToolFrames(Vec<(String, Mat)>);

impl ToolFrames {
    /// For a feature with drawn tools (see [`drawn_tools_change_bodies`]): the bodies placed
    /// differently from its component, so its tools meet them where they are shown (#3383).
    fn install(doc: &Document, f: &Feature, st: &ModelState) -> Option<ToolFrames> {
        if !drawn_tools_change_bodies(&f.kind) || (doc.occurrences.is_empty() && doc.body_offsets.is_empty()) {
            return None;
        }
        let frame = doc.component_transform(f.component);
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

pub fn state_in_frame<'a>(doc: &Document, st: &'a ModelState, comp: u64) -> std::borrow::Cow<'a, ModelState> {
    state_in_frame_at(doc, st, comp, doc.component_transform(comp))
}

/// The model a feature's face points (`FeatureKind::face_points`) are found in. Extrude's To /
/// From and Revolve's To place geometry, so they are kept in the feature's own component frame
/// (`comp`): the model as that component sees it, a target in another, moved component where it
/// is shown (#928). Other face points are picks, kept in the frame of the body they lie on: the
/// model itself.
pub fn face_point_state<'a>(doc: &Document, st: &'a ModelState, kind: &FeatureKind, comp: u64) -> std::borrow::Cow<'a, ModelState> {
    match kind {
        FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. } => state_in_frame(doc, st, comp),
        _ => std::borrow::Cow::Borrowed(st),
    }
}

/// The names of the faces a feature's face points lie on (empty where none), found in
/// [`face_point_state`]. A To / From point is named after the nearest face over every body, the
/// one `object_height` resolves it to, not the first body with a face within tolerance: a part
/// resting a hair from the picked face would otherwise take its name.
pub fn face_point_names(doc: &Document, st: &ModelState, kind: &FeatureKind, comp: u64) -> Vec<String> {
    let pts = kind.face_points();
    if !matches!(kind, FeatureKind::Extrude { .. } | FeatureKind::Revolve { .. }) {
        return crate::naming::face_names_at(st, &pts);
    }
    let fst = face_point_state(doc, st, kind, comp);
    let normal = match kind {
        FeatureKind::Extrude { sketch, .. } => {
            st.sketch(sketch.0).and_then(|ss| sketch_in_feature_frame(doc, ss, comp).ok()).map(|ss| ss.plane.normal())
        }
        _ => None,
    };
    pts.iter()
        .map(|p| {
            fst.bodies
                .iter()
                .filter(|b| !b.body.is_mesh())
                .filter_map(|b| {
                    let (d, fi) = nearest_object_face(b, *p, normal)?;
                    (d <= (b.body.size() * 1e-3).max(1e-3)).then_some((d, b, fi))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .and_then(|(_, b, fi)| crate::naming::face_names(b).get(fi).cloned())
                .unwrap_or_default()
        })
        .collect()
}

/// As `state_in_frame`, using the exact active placement for its shared source geometry.
pub fn state_in_frame_at<'a>(doc: &Document, st: &'a ModelState, comp: u64, frame: Mat) -> std::borrow::Cow<'a, ModelState> {
    use std::borrow::Cow;
    if doc.occurrences.is_empty() && doc.body_offsets.is_empty() {
        return Cow::Borrowed(st);
    }
    let Some(inv) = crate::mat_inverse(&frame) else { return Cow::Borrowed(st) };
    let comp_of_feature = |id: u64| doc.feature(id).map(|f| f.component).unwrap_or(0);
    let rel = |c: u64, body: Option<&str>| {
        let m = crate::mat_mul(&inv, &if c == comp { frame } else { doc.component_transform(c) });
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
        return Cow::Borrowed(st);
    }
    let mut out = st.clone();
    for (b, m) in out.bodies.iter_mut().zip(&body_m) {
        if !crate::is_identity(m)
            && let Ok(moved) = st.body_in_frame(b, m)
        {
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
        if !crate::is_identity(m)
            && let Some(np) = Plane::new(crate::apply_point(m, s.plane.origin), crate::apply_vector(m, s.plane.x), crate::apply_vector(m, s.plane.y))
        {
            s.plane = np;
        }
        for wire in &mut s.sketch.wires {
            let matrix = if wire.space == solidcraft_sketch::WireSpace::Component { m } else { &inv };
            map_wire(wire, matrix);
        }
    }
    Cow::Owned(out)
}

/// The model placed in the world: each component's bodies, sketches, threads and construction
/// geometry through every occurrence of it (bodies of the first placement keep their names;
/// further instances get the occurrence path in brackets). Without components this is the model
/// itself.
pub fn world_state(doc: &Document, st: &ModelState) -> ModelState {
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

    /// The body `b` placed by `m`, from an earlier call when `b` is the same body as then.
    fn placed(&self, b: &ModelBody, m: &crate::Mat, name: String) -> ModelBody {
        if let Some(placed) = self.taken(b, m, &name) {
            return placed;
        }
        match kernel::place_matrix(&b.body, *m) {
            Ok(body) => {
                let mut fresh = ModelBody::new(name, body, b.feature);
                fresh.mesh_placement = placed_mesh::PlacedMesh::new(b, m).map(Arc::new);
                fresh
            }
            Err(_) => ModelBody::new(name, b.body.clone(), b.feature),
        }
    }

    /// The body `b` placed by `m` as an earlier call placed it, when `b` is the same body as then.
    fn taken(&self, b: &ModelBody, m: &crate::Mat, name: &str) -> Option<ModelBody> {
        self.reused(b, m).map(|placed| {
            let mut fresh = ModelBody::new(name.to_string(), placed.body.clone(), b.feature);
            fresh.mesh = placed.mesh.clone();
            fresh.measured = placed.measured.clone();
            fresh.mesh_placement = placed.mesh_placement.clone();
            fresh.plan = placed.plan.clone();
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
    moved.plan_mesh = placed.plan_mesh.clone();
    moved.component = b.component;
    moved.transferred = b.transferred;
    moved.source_name = b.source_name.clone();
    moved.source_transform = crate::mat_mul(m, &b.source_transform);
    moved.imported = b.imported.clone();
    // Moving a body keeps its faces (and their order): it keeps their names.
    moved.names = crate::naming::NameCell::same(b);
    moved
}

fn placed_key(b: &ModelBody, m: &crate::Mat) -> (usize, [u64; 16]) {
    let mut bits = [0u64; 16];
    for (slot, v) in bits.iter_mut().zip(m.iter().flatten()) {
        *slot = v.to_bits();
    }
    (Arc::as_ptr(&b.mesh) as usize, bits)
}

/// [`world_state`], reusing the bodies `placed` holds from an earlier call and leaving it holding
/// this call's.
pub fn world_state_reusing(doc: &Document, st: &ModelState, placed_before: &mut PlacedBodies) -> ModelState {
    if doc.occurrences.is_empty() {
        return st.clone();
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
    let placements = doc.placements();
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
            if !crate::is_identity(&bm) {
                jobs.push((b, bm, names.name(&b.name, *comp, path)));
            }
        }
    }
    // Bodies placed as before are taken here: starting threads only to copy them cost every
    // command in a design with placed components more than the copies.
    let reused: Vec<_> = jobs.iter().map(|(b, bm, name)| placed_before.taken(b, bm, name)).collect();
    let fresh: Vec<_> = jobs.iter().zip(&reused).filter(|(_, r)| r.is_none()).map(|(job, _)| job).collect();
    let mut placed_fresh = kernel::map_on_threads(&fresh, 16, |(b, bm, name)| placed_before.placed(b, bm, name.clone())).into_iter();
    let mut moved = reused.into_iter().map(|r| r.or_else(|| placed_fresh.next().flatten())).zip(&jobs);
    for (comp, path, m) in placements {
        let primary = path == first_path(comp);
        let ident = crate::is_identity(&m);
        for b in bodies_of(comp) {
            let bm = body_matrix(b, &m);
            // Identical geometry also shares its display mesh cell with the definition.
            // A changed placement keeps a distinct mesh, at that placement's coordinates.
            let mut placed = if crate::is_identity(&bm) {
                let mut placed = b.clone();
                placed.name = names.name(&b.name, comp, &path);
                placed
            } else {
                // The same order as the jobs: this body's, made above (again here if it failed).
                let placed = match moved.next() {
                    Some((Some(placed), _)) => placed,
                    Some((None, (_, _, name))) => placed_before.placed(b, &bm, name.clone()),
                    None => placed_before.placed(b, &bm, names.name(&b.name, comp, &path)),
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
                if !ident {
                    let p = &s.plane;
                    if let Some(np) = Plane::new(crate::apply_point(&m, p.origin), crate::apply_vector(&m, p.x), crate::apply_vector(&m, p.y)) {
                        s2.plane = np;
                    }
                }
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
            t2.axis = crate::apply_vector(&frame, *t.axis).unit().unwrap_or(t.axis);
            out.threads.push(t2);
        }
    }
    *placed_before = placed_now;
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
        let world = world_state(&doc, &state);
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
        let world = world_state(&doc, &state);
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
        let again = world_state(&doc, &state);
        assert!(Arc::ptr_eq(&original_mesh, &again.body("Block").unwrap().mesh()));
        assert!(Arc::ptr_eq(&names, &crate::naming::face_names(again.body(&second_name).unwrap())));
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
}

#[cfg(test)]
#[path = "eval_cost_tests.rs"]
pub(crate) mod cost_tests;

#[cfg(test)]
#[path = "eval_whole_face_tests.rs"]
mod whole_face_tests;

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
}

#[cfg(test)]
mod coplanar_growth_tests {
    use super::*;

    #[test]
    fn buried_join_ends_stop_at_material_boundaries() {
        let block = kernel::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 8.0)).unwrap();
        let profile = rect(2.0, 2.0, 4.0, 4.0);
        let st = state(vec![block.clone()]);
        let sur = Surroundings::of(&st, Operation::Join, &[]).unwrap();
        assert_eq!(trim_embedded_join_ends(&sur, &Plane::XY, &profile, 0.0, 16.0), (8.0, 16.0));
        assert_eq!(trim_embedded_join_ends(&sur, &Plane::XY, &profile, -8.0, 8.0), (-8.0, 0.0));
        // A thin sealed void between the old start and the rib's top must still
        // be filled by the join. The remaining tool must start before that void.
        let void = kernel::box_solid(Vec3::new(1.0, 1.0, 3.01), Vec3::new(5.0, 5.0, 3.02)).unwrap();
        let hollow = kernel::boolean(&block, &void, BoolOp::Cut).unwrap().unwrap();
        let st = state(vec![hollow]);
        let sur = Surroundings::of(&st, Operation::Join, &[]).unwrap();
        let (lo, hi) = trim_embedded_join_ends(&sur, &Plane::XY, &profile, 0.0, 16.0);
        assert!(lo <= 3.01, "trimming past {lo} would discard material the join adds in the void");
        assert_eq!(hi, 16.0);
        let cut = Surroundings::of(&st, Operation::Cut, &[]).unwrap();
        assert_eq!(trim_embedded_join_ends(&cut, &Plane::XY, &profile, 0.0, 16.0), (0.0, 16.0));
    }

    fn state(bodies: Vec<Body>) -> ModelState {
        ModelState { bodies: bodies.into_iter().enumerate().map(|(i, b)| ModelBody::new(format!("Body{i}"), b, 0)).collect(), ..Default::default() }
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
        let p = Vec2::new;
        Region2 { outer: solidcraft_geom::Loop2::polygon(&[p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)]), holes: Vec::new() }
    }

    /// The rectangle `x0` … `x1` by `y0` … `y1` with its corners rounded to `rad`, counterclockwise.
    fn rounded(x0: f64, y0: f64, x1: f64, y1: f64, rad: f64) -> Region2 {
        use solidcraft_geom::Seg2;
        let p = Vec2::new;
        let quarter = std::f64::consts::FRAC_PI_2;
        let arc = |cx: f64, cy: f64, k: f64| Seg2::Arc { center: p(cx, cy), radius: rad, start: k * quarter, sweep: quarter };
        let segs = vec![
            Seg2::Line { a: p(x0 + rad, y0), b: p(x1 - rad, y0) },
            arc(x1 - rad, y0 + rad, 3.0),
            Seg2::Line { a: p(x1, y0 + rad), b: p(x1, y1 - rad) },
            arc(x1 - rad, y1 - rad, 0.0),
            Seg2::Line { a: p(x1 - rad, y1), b: p(x0 + rad, y1) },
            arc(x0 + rad, y1 - rad, 1.0),
            Seg2::Line { a: p(x0, y1 - rad), b: p(x0, y0 + rad) },
            arc(x0 + rad, y0 + rad, 2.0),
        ];
        Region2 { outer: solidcraft_geom::Loop2 { segs }, holes: Vec::new() }
    }

    /// The extrude's growth of `r` (a cut from the plane z = `z`, `lo` … `hi` along +z), as
    /// `feature_tools_with_constructor` asks for it.
    fn grown(st: &ModelState, z: f64, r: &Region2, lo: f64, hi: f64) -> (Region2, f64, f64) {
        let sur = Surroundings::of(st, Operation::Cut, &[]).unwrap();
        let plane = Plane { origin: Vec3::new(0.0, 0.0, z), ..Plane::XY };
        let (l2, h2) = extend_for_coplanar(&sur, &plane, r, lo, hi);
        let probe = |p: Vec2| (1..4).map(|k| plane.to_world(p) + Vec3::Z * (lo + (hi - lo) * k as f64 / 4.0)).collect::<Vec<_>>();
        let gains = |pieces: &[[Vec2; 3]]| sur.harmless(&plane, pieces, -sur.skin, l2 + sur.skin, h2 - sur.skin);
        (grow_profile_for_coplanar(&sur, r, &probe, &gains), l2, h2)
    }

    /// A cut's tool still grows past the faces it lies on where all it gains is empty space: a
    /// notch 10 deep into the corner of a 100 × 60 × 30 block, flush with two of its sides,
    /// grows out of those sides by 0.5 % of the block's size and up out of its top by 1 %.
    /// Cut into the end wall of a tray of that size, flush with the wall's inner face, the
    /// strip its side would gain holds the start of the long walls: there it stays as drawn.
    #[test]
    fn the_tool_grows_only_where_it_gains_nothing() {
        let block = kernel::box_solid(Vec3::ZERO, Vec3::new(100.0, 60.0, 30.0)).unwrap();
        let st = state(vec![block.clone()]);
        let notch = rect(0.0, 0.0, 20.0, 20.0);
        let (r, lo, hi) = grown(&st, 30.0, &notch, -10.0, 0.0);
        let step = block.size() * 0.005;
        for p in [Vec2::new(-0.9 * step, 10.0), Vec2::new(10.0, -0.9 * step), Vec2::new(-0.9 * step, -0.9 * step)] {
            assert!(r.contains(p) && !notch.contains(p), "{p:?}: {r:?}");
        }
        assert!(!r.contains(Vec2::new(20.0 + 0.1 * step, 10.0)) && !r.contains(Vec2::new(10.0, 20.0 + 0.1 * step)), "{r:?}");
        assert_eq!(lo, -10.0);
        assert!((hi - block.size() * 0.01).abs() < 1e-9, "{hi}");

        let inner = kernel::box_solid(Vec3::new(2.0, 2.0, 2.0), Vec3::new(98.0, 58.0, 40.0)).unwrap();
        let tray = kernel::boolean(&block, &inner, BoolOp::Cut).unwrap().unwrap();
        let st = state(vec![tray]);
        let end_wall = rect(-1.0, -1.0, 2.0, 70.0);
        assert_eq!(grown(&st, 2.0, &end_wall, 0.0, 40.0), (end_wall, 0.0, 40.0));
    }

    /// Arcs grow too (#1275), on the same terms. A cut of the whole top face of a 40 × 40 × 20
    /// block with corners rounded to 8 grows out of every side, its corners' arcs by the same step
    /// as its edges. A post standing 0.1 off one corner's arc, between the points the samples
    /// look at, would lose a sliver to the grown arc: there the loop stays as drawn.
    #[test]
    fn arcs_grow_only_where_they_gain_nothing() {
        let face = rounded(0.0, 0.0, 40.0, 40.0, 8.0);
        let block = kernel::extrude(&Plane::XY, std::slice::from_ref(&face), 0.0, 20.0).unwrap().remove(0);
        let step = block.size() * 0.005;
        let st = state(vec![block.clone()]);
        let (r, _, _) = grown(&st, 20.0, &face, -5.0, 0.0);
        let corner = |k: f64| Vec2::new(8.0, 8.0) + Vec2::from_angle(1.25 * std::f64::consts::PI) * (8.0 + k * step);
        for q in [Vec2::new(-0.9 * step, 20.0), Vec2::new(20.0, 40.0 + 0.9 * step), corner(0.9)] {
            assert!(r.contains(q) && !face.contains(q), "{q:?}: {r:?}");
        }
        assert!(!r.contains(corner(1.1)), "{r:?}");

        // The post's corner nearest the arc's centre (8, 8) lies 8.1 from it, at 190 degrees; the
        // samples look at 202.5, 225 and 247.5 degrees.
        let post = kernel::box_solid(Vec3::new(-2.0, 5.0, 0.0), Vec3::new(8.0 - 7.977, 8.0 - 1.407, 20.0)).unwrap();
        let st = state(vec![block, post]);
        assert_eq!(grown(&st, 20.0, &face, -5.0, 0.0).0, face);
    }
}

/// The parts of a body split by a plane, on its positive side first (only the non-empty ones).
/// Each part's cut face is named `section` after the half-space box that cut it, so it takes an
/// attachment that follows the plane (#565); the box's other faces never reach the part.
fn split_by_plane_traced(input: &Traced, plane: &Plane) -> Result<Vec<Traced>> {
    let mut parts = Vec::new();
    for positive in [true, false] {
        let (part, tool) = kernel::half_space(&input.body, plane, positive)?;
        let Some(part) = part else { continue };
        // The box's cap in the plane: its start cap on the positive side, its end cap on the other.
        let section = Some("section".to_string());
        let caps = if positive { [section, None] } else { [None, section] };
        let tool = crate::lineage::ProfileRoles { sketch: None, region: None, caps, thin: None }.tool(tool, None);
        parts.push(Traced::step(part, &[input, &tool]));
    }
    Ok(parts)
}

/// The plane a straight sketch line sweeps along its sketch's normal: through the line, its x
/// along the line and its y along the normal (a Split Body tool, #1552).
pub fn sketch_line_plane(st: &ModelState, sketch: u64, line: &str) -> Result<Plane> {
    let ss = st.sketch(sketch).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
    let ci = ss.sketch.curve_index(line).ok_or_else(|| DocError::Unknown(format!("curve `{line}`")))?;
    let (a, b) = match ss.sketch.segs(ci).as_slice() {
        [solidcraft_geom::Seg2::Line { a, b }] => (*a, *b),
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
        assert_eq!((h.body, h.split, h.merged), (7, false, false));
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
        assert_eq!((moved.body, moved.split), (2, false));
    }

    /// A copy of a body has its source's face names. Over every body the name finds the face on
    /// both, and the one nearest the point is taken as a piece of a split face. A feature on the
    /// copy takes the copy's face, whole.
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
        assert_eq!((on_copy.split, everywhere.split), (false, true));
        assert_eq!(on_copy.note(&name), None);
    }
}
