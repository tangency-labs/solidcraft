use serde::Serialize;
use solidcraft_geom::brep::{self, Brep, Curve, Loop, Shell, Surface};
use solidcraft_geom::{Aabb3, Mesh, Vec3};

use crate::history::BodyHistory;
use crate::{KernelError, Result, guard};

#[path = "planar_regularize.rs"]
mod planar_regularize;
#[path = "planar_sources.rs"]
mod planar_sources;
#[path = "shell_groups.rs"]
pub(crate) mod shell_groups;

/// A solid body (one or more closed shells), or a mesh body (triangles only, like an imported
/// 3MF or STL: it renders, measures and exports, but B-rep features cannot use it). Cheap to
/// clone; never mutated in place.
///
/// A solid body is its schema 4 record ([`solidcraft_geom::brep`]): faces, edges and vertices
/// in the record's canonical order are the body's own, everywhere (history, naming, meshes).
/// SMLib works on the same body built from that record, made once and shared by clones.
#[derive(Clone, Debug)]
pub struct Body {
    pub(crate) record: std::sync::Arc<Brep>,
    /// The body in SMLib, built from `record` on first use (or kept from the operation that
    /// made it), shared by clones because they share the record.
    pub(crate) smlib: std::sync::Arc<std::sync::OnceLock<std::result::Result<solidcraft_smlib::Brep, String>>>,
    /// Bounding-box diagonal of the vertices, measured when the body was made; 0 means "not
    /// measured, walk the geometry". Every kernel tolerance is a fraction of it.
    pub(crate) size: f64,
    /// The last volume measured for this record and the tolerance it was measured at, shared by
    /// clones because they share the record.
    pub(crate) volume: Option<std::sync::Arc<std::sync::Mutex<Option<(u64, f64)>>>>,
    /// Set for mesh bodies (`record` is then empty).
    pub(crate) mesh: Option<std::sync::Arc<crate::meshbody::TriMesh>>,
    /// Display colour (linear RGB 0..1) from an imported file; operations drop it.
    pub(crate) color: Option<[f32; 3]>,
    /// Opacity and face colours for export (set by the document; operations drop it).
    pub(crate) paint: Option<std::sync::Arc<Paint>>,
    /// Lines the user split faces along (Split Face), as polylines: export keeps the faces on
    /// either side apart where it would otherwise join pieces of one surface.
    pub(crate) splits: Option<std::sync::Arc<Vec<Vec<Vec3>>>>,
    /// Exact construction/import source identities, in this body's face order.
    pub(crate) face_sources: Option<std::sync::Arc<Vec<Option<FaceSource>>>>,
    /// Sticky transient veto: authored face boundaries must survive later rebuilds.
    pub(crate) regularization_protected: bool,
    /// Exact history of the operation that made this body, relative to that operation's
    /// operands; `None` when the operation does not report one.
    pub(crate) history: Option<std::sync::Arc<crate::history::History>>,
    /// A placed copy of a body that another body shows already (an instance of a part a STEP
    /// assembly lists many times): its display mesh is that body's, placed by the matrix, so the
    /// part is meshed once rather than once per instance. Dropped by anything that changes what
    /// the body shows (its geometry is never changed in place).
    pub(crate) display_of: Option<std::sync::Arc<(std::sync::Arc<SharedDisplay>, [[f64; 4]; 4])>>,
    /// Meshes of this record kept for reuse, set by [`Body::keeping_meshes`]: a check that
    /// intersects one large body with many small ones meshes it at the same tolerance each time.
    pub(crate) meshes: Option<std::sync::Arc<MeshMemo>>,
}

/// The meshes made of one record (see [`Body::keeping_meshes`]), by tolerance and angle. They
/// answer only for that record: a copy whose record was replaced makes its own.
#[derive(Debug)]
pub(crate) struct MeshMemo {
    record: std::sync::Arc<Brep>,
    made: Remembered,
}

/// Meshes a [`MeshMemo`] keeps at most (the oldest goes first).
const MEMO_MESHES: usize = 4;

/// Meshes kept for reuse, by tolerance (see [`remembered`]).
type Remembered = std::sync::Mutex<Vec<((u64, u64), std::sync::Arc<Mesh>)>>;

/// The mesh at `t` from `kept`, made by `make` and kept (at most [`MEMO_MESHES`]) if it isn't
/// there. The lock is held while meshing: threads asking for the same mesh wait for it rather
/// than make it too.
fn remembered(kept: &Remembered, t: &MeshTolerance, make: impl FnOnce() -> Result<Mesh>) -> Result<std::sync::Arc<Mesh>> {
    let Ok(mut made) = kept.lock() else { return make().map(std::sync::Arc::new) };
    let key = t.key();
    if let Some((_, m)) = made.iter().find(|(k, _)| *k == key) {
        return Ok(m.clone());
    }
    let m = std::sync::Arc::new(make()?);
    if made.len() >= MEMO_MESHES {
        made.remove(0);
    }
    made.push((key, m.clone()));
    Ok(m)
}

/// The display mesh of a body that several placed copies show (see `Body::display_of`): the
/// body, and its mesh once made.
#[derive(Debug)]
pub(crate) struct SharedDisplay {
    pub(crate) original: Body,
    meshes: Remembered,
}

impl SharedDisplay {
    pub(crate) fn new(original: Body) -> Self {
        SharedDisplay { original: Body { display_of: None, meshes: None, ..original }, meshes: std::sync::Mutex::new(Vec::new()) }
    }
}

/// The display one body shares with its placed copies (see [`Body::display_share`]), so a
/// saved design can store the part once and give its copies the same display again.
#[derive(Clone, Debug)]
pub struct DisplayShare(std::sync::Arc<SharedDisplay>);

impl DisplayShare {
    pub fn new(original: Body) -> Self {
        DisplayShare(std::sync::Arc::new(SharedDisplay::new(original)))
    }
    /// The body whose display the copies show.
    pub fn original(&self) -> &Body {
        &self.0.original
    }
    /// Identity: copies that show the same display have the same id.
    pub fn id(&self) -> usize {
        std::sync::Arc::as_ptr(&self.0) as usize
    }
}

/// `m` placed by the rigid matrix `t` (by columns, translation in `t[3]`, as the kernel's
/// transforms take it): positions, normals and edge polylines move, everything indexed stays.
/// `None` for anything but a rotation and translation (a reflection would turn the triangles
/// inside out).
fn placed_mesh(m: &Mesh, t: &[[f64; 4]; 4]) -> Option<Mesh> {
    let r = |v: Vec3, w: f64| {
        Vec3::new(
            t[0][0] * v.x + t[1][0] * v.y + t[2][0] * v.z + w * t[3][0],
            t[0][1] * v.x + t[1][1] * v.y + t[2][1] * v.z + w * t[3][1],
            t[0][2] * v.x + t[1][2] * v.y + t[2][2] * v.z + w * t[3][2],
        )
    };
    brep::placement::Rigid::of_matrix(t)?;
    Some(Mesh {
        positions: m.positions.iter().map(|p| r(*p, 1.0)).collect(),
        normals: m.normals.iter().map(|n| r(*n, 0.0)).collect(),
        edges: m.edges.iter().map(|e| e.iter().map(|p| r(*p, 1.0)).collect()).collect(),
        ..m.clone()
    })
}


#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FaceSource {
    ExtrudeSide {
        segment: solidcraft_geom::Seg2,
        plane: Option<solidcraft_geom::Plane>,
        coalesced: bool,
    },
    CertifiedExtrudeSide {
        producer: u64,
        sketch: u64,
        curve: String,
        plane: Option<solidcraft_geom::Plane>,
    },
    ExtrudeCap {
        high: bool,
        region: solidcraft_geom::Region2,
        plane: solidcraft_geom::Plane,
    },
    CertifiedExtrudeCap {
        producer: u64,
        sketch: u64,
        high: bool,
        outer: Vec<String>,
        holes: Vec<Vec<String>>,
        plane: solidcraft_geom::Plane,
    },
    Box {
        producer: Option<u64>,
        axis: u8,
        high: bool,
        plane: solidcraft_geom::Plane,
    },
    /// A cylinder primitive's cap: `high` at the far end (its `top`), else at the base.
    CylinderCap {
        producer: u64,
        high: bool,
        plane: solidcraft_geom::Plane,
    },
    ShellPlanar {
        producer: Option<u64>,
        source: Box<FaceSource>,
        inner: bool,
        plane: solidcraft_geom::Plane,
    },
    Imported {
        item: u64,
        face: u64,
        payload_sha256: Option<String>,
        producer: Option<u64>,
    },
}

pub type CapSignature = (Vec<String>, Vec<Vec<String>>);
type FaceSources = std::sync::Arc<Vec<Option<FaceSource>>>;

impl FaceSource {
    fn plane_mut(&mut self) -> Option<&mut solidcraft_geom::Plane> {
        match self {
            Self::ExtrudeSide { plane, .. } | Self::CertifiedExtrudeSide { plane, .. } => plane.as_mut(),
            Self::ExtrudeCap { plane, .. }
            | Self::CertifiedExtrudeCap { plane, .. }
            | Self::Box { plane, .. }
            | Self::CylinderCap { plane, .. }
            | Self::ShellPlanar { plane, .. } => Some(plane),
            Self::Imported { .. } => None,
        }
    }
}

/// How a body looks beyond its colour, for export: its opacity and its faces' own colours.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Paint {
    /// 1 = opaque.
    pub opacity: f32,
    pub faces: Vec<FacePaint>,
}

/// A face's own colour: the B-rep face index (`face_iter` order), sRGB as 0..1, opacity.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FacePaint {
    pub face: usize,
    pub color: [f32; 3],
    pub opacity: f32,
}

/// A B-rep edge as seen by the rest of the application.
#[derive(Clone, Debug, Serialize)]
pub struct EdgeInfo {
    pub index: usize,
    /// Polyline approximation.
    pub points: Vec<Vec3>,
    /// Point halfway along the polyline (used to re-find the edge).
    pub mid: Vec3,
    pub length: f64,
}

/// A B-rep face.
#[derive(Clone, Debug, Serialize)]
pub struct FaceInfo {
    pub index: usize,
    pub area: f64,
    pub centroid: Vec3,
    /// Unit normal if the face is planar.
    pub plane_normal: Option<Vec3>,
}


/// The most one facet of the display mesh turns (radians): a curved face or edge is divided
/// until no facet spans more of it than this, however small its radius is beside the body.
/// 20 degrees: a quarter turn is drawn with at least five facets, a full circle with at least 18.
pub const DISPLAY_ANGLE: f64 = std::f64::consts::PI / 9.0;

/// The sag, as a fraction of the chord tolerance, below which the angle no longer divides a
/// facet: at [`DISPLAY_ANGLE`] and the application's tolerance of 1/1000 of the body, a radius
/// under about 1/15000 of the body gets fewer facets than the angle asks for, and one under
/// a millionth of it only those of the chord tolerance.
const DISPLAY_ANGLE_FLOOR: f64 = 1e-3;

/// The finest angle a display mesh is divided by (one degree).
const MIN_DISPLAY_ANGLE: f64 = std::f64::consts::PI / 180.0;

/// How closely [`Body::mesh`] follows the exact surfaces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshTolerance {
    /// The farthest a facet or an edge segment may stand off the surface or curve it stands for
    /// (mm). Not finite or not positive: 0.05 mm. Never finer than a millionth of the body.
    pub chord: f64,
    /// The most a facet may turn (radians, at least one degree) where its sag is above a
    /// thousandth of `chord`; `None`: `chord` alone.
    pub angle: Option<f64>,
}

impl MeshTolerance {
    /// `chord` alone.
    pub const fn chord(chord: f64) -> Self {
        MeshTolerance { chord, angle: None }
    }

    /// What the viewport draws: `chord`, and no facet turning more than [`DISPLAY_ANGLE`], so
    /// small radii on a large body look round too.
    pub const fn display(chord: f64) -> Self {
        MeshTolerance { chord, angle: Some(DISPLAY_ANGLE) }
    }

    /// The application's display mesh of a body of size `size` (its box diagonal): 1/1000 of
    /// it, at least 1 µm, at [`DISPLAY_ANGLE`].
    pub fn display_of(size: f64) -> Self {
        MeshTolerance::display((size * 1e-3).max(1e-3))
    }

    /// For export to a mesh format (STL, 3MF, OBJ) of a body of size `size`: 1/5000 of it, at
    /// least 1 µm, the chord alone.
    pub fn export_of(size: f64) -> Self {
        MeshTolerance::chord((size * 2e-4).max(1e-3))
    }

    /// The planner's chord tolerance and facet angle for a body of size `size`.
    pub(crate) fn rules(&self, size: f64) -> (f64, Option<crate::tess::size::Turn>) {
        let tol = if self.chord.is_finite() && self.chord > 0.0 { self.chord } else { 0.05 };
        let tol = tol.max(size * 1e-6);
        let turn = self
            .angle
            .filter(|a| a.is_finite() && *a > 0.0)
            .map(|a| crate::tess::size::Turn { angle: a.max(MIN_DISPLAY_ANGLE), floor: tol * DISPLAY_ANGLE_FLOOR });
        (tol, turn)
    }

    /// The key a kept mesh is found by.
    fn key(&self) -> (u64, u64) {
        (self.chord.to_bits(), self.angle.map_or(u64::MAX, f64::to_bits))
    }
}

pub(crate) fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

pub(crate) fn polyline_mid(pts: &[Vec3]) -> (Vec3, f64) {
    let len: f64 = pts.windows(2).map(|w| w.first().zip(w.get(1)).map(|(a, b)| a.dist(*b)).unwrap_or(0.0)).sum();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
            let l = a.dist(*b);
            if acc + l >= len * 0.5 && l > 0.0 {
                return (a.lerp(*b, (len * 0.5 - acc) / l), len);
            }
            acc += l;
        }
    }
    (pts.first().copied().unwrap_or_default(), len)
}

/// Deflection of the coarse mesh `unmeshed_faces` counts on: coarse enough for a large body,
/// fine enough that a small one still meshes.
fn coarse_tolerance(span: f64) -> f64 {
    (span * 0.01).clamp(1e-3, 1.0)
}

/// Box diagonal of a record's vertices (0 when it has none or no finite extent).
fn span_of(record: &Brep) -> f64 {
    let bounds: Aabb3 = record.vertices.iter().map(|v| v3(v.point)).collect();
    let d = if bounds.is_empty() { 0.0 } else { bounds.diagonal() };
    if d.is_finite() && d > 0.0 { d } else { 0.0 }
}

/// Equal shared values: the same allocation (a clone of the body), else equal contents.
fn same_shared<T: PartialEq>(a: &Option<std::sync::Arc<T>>, b: &Option<std::sync::Arc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b) || a == b,
        (None, None) => true,
        _ => false,
    }
}

/// `record`, checked to be a body's: valid structure (budgets, references, loops, regions,
/// canonical order) and some bounded material.
fn solid_record(record: Brep) -> Result<Brep> {
    record.validate().map_err(|e| KernelError::Invalid(e.to_string()))?;
    if record.faces.is_empty() || record.material_regions().is_empty() {
        return Err(KernelError::Failed("empty result".into()));
    }
    Ok(record)
}

/// The record of no solid: only the infinite region.
pub(crate) fn empty_record() -> Brep {
    Brep {
        version: brep::VERSION,
        tolerance: 1e-6,
        regions: vec![brep::Region { shells: Vec::new(), void: false }],
        shells: Vec::new(),
        faces: Vec::new(),
        edges: Vec::new(),
        vertices: Vec::new(),
    }
}

impl Body {
    /// A body with `record` and nothing else set.
    fn bare(record: Brep, smlib: Option<solidcraft_smlib::Brep>, mesh: Option<std::sync::Arc<crate::meshbody::TriMesh>>) -> Body {
        let size = span_of(&record);
        let cell = std::sync::OnceLock::new();
        if let Some(handle) = smlib {
            let _ = cell.set(Ok(handle));
        }
        Body {
            record: std::sync::Arc::new(record),
            smlib: std::sync::Arc::new(cell),
            size,
            volume: Some(std::sync::Arc::new(std::sync::Mutex::new(None))),
            mesh,
            color: None,
            paint: None,
            splits: None,
            face_sources: None,
            regularization_protected: false,
            history: None,
            display_of: None,
            meshes: None,
        }
    }

    /// The solid body of a schema 4 record: checked for structure (budgets, references, loops,
    /// regions, canonical order) and for bounding material. Its faces face as the record says.
    pub(crate) fn from_record(record: Brep) -> Result<Body> {
        Ok(Body::bare(solid_record(record)?, None, None))
    }

    /// The body of an SMLib B-rep and its record (SMLib's export, in canonical order), keeping
    /// the SMLib body so later operations need not build it again.
    pub(crate) fn from_smlib_record(handle: solidcraft_smlib::Brep, record: Brep) -> Result<Body> {
        Ok(Body::bare(solid_record(record)?, Some(handle), None))
    }

    /// A mesh body of `mesh`.
    pub(crate) fn of_mesh(mesh: crate::meshbody::TriMesh) -> Body {
        Body::bare(empty_record(), None, Some(std::sync::Arc::new(mesh)))
    }

    /// The body's record: faces, edges and vertices in the body's own order. Empty for a mesh body.
    pub fn record(&self) -> &Brep {
        &self.record
    }

    /// The body in SMLib, built from its record the first time it is asked for. The record is
    /// SMLib's own form (`SmBrepData`), imported as given: a record SMLib refuses is an error that
    /// names why, never repaired here.
    pub fn smlib(&self) -> Result<&solidcraft_smlib::Brep> {
        self.require_brep("an SMLib operation")?;
        self.smlib
            .get_or_init(|| solidcraft_smlib::Brep::from_record(&self.record, true).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| KernelError::Failed(format!("SMLib: {e}")))
    }

    /// The very same body (shared data), not just an equal-looking one. Its history (how an
    /// operation made it) is not part of it: an operation that leaves a body as it is (a cut by
    /// a tool clear of it) gives the same body, with that operation's history.
    pub fn same(&self, other: &Body) -> bool {
        self.same_shape(other)
            && self.color == other.color
            && same_shared(&self.paint, &other.paint)
            && same_shared(&self.splits, &other.splits)
            && same_shared(&self.face_sources, &other.face_sources)
            && self.regularization_protected == other.regularization_protected
    }

    /// The very same shape (shared record or mesh), whatever its colour, paint or face splits.
    pub fn same_shape(&self, other: &Body) -> bool {
        std::sync::Arc::ptr_eq(&self.record, &other.record)
            && match (&self.mesh, &other.mesh) {
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }

    /// Whether face `index`'s normal in its body is its surface's natural normal (its first
    /// oriented face's `forward`, the side the record walks it by).
    pub(crate) fn face_forward(&self, index: usize) -> bool {
        self.record
            .shells
            .iter()
            .filter_map(|s| match s {
                Shell::Faces { faces, .. } => Some(faces),
                _ => None,
            })
            .flatten()
            .find(|f| f.face as usize == index)
            .is_none_or(|f| f.forward)
    }

    pub(crate) fn inherit_regularization_protection(mut self, input: &Body) -> Self {
        self.regularization_protected |= input.regularization_protected;
        self
    }

    /// Exact history of the operation that made this body, relative to that operation's
    /// operands (see [`crate::history`]); `None` when the operation does not report one.
    pub fn history(&self) -> Option<&crate::history::History> {
        self.history.as_deref()
    }

    /// Attach the history of the operation that made this body from `operands`, after checking
    /// it describes this body.
    pub(crate) fn with_history(mut self, history: crate::history::History, operands: &[&Body]) -> Result<Self> {
        history.check(&self, operands)?;
        self.history = Some(std::sync::Arc::new(history));
        Ok(self)
    }

    /// [`Body::with_history`], given the operands' face and edge counts.
    pub(crate) fn with_history_counts(mut self, history: crate::history::History, operands: &[(usize, usize)]) -> Result<Self> {
        history.check_counts((self.face_count(), self.edge_count()), operands)?;
        self.history = Some(std::sync::Arc::new(history));
        Ok(self)
    }

    /// This body with `history` if it describes the body made from operands with these face
    /// and edge counts, else with none: an operation whose history could not be recorded still
    /// gives its result.
    pub(crate) fn with_checked_history(mut self, history: Option<crate::history::History>, operands: &[(usize, usize)]) -> Self {
        let counts = (self.face_count(), self.edge_count());
        self.history = history.and_then(|h| match h.check_counts(counts, operands) {
            Ok(()) => Some(std::sync::Arc::new(h)),
            Err(e) => {
                log::debug!("no history: {e}");
                None
            }
        });
        self
    }

    /// This body with no history: an operation whose history is not known.
    pub(crate) fn without_history(mut self) -> Self {
        self.history = None;
        self
    }

    pub fn face_sources(&self) -> Option<&[Option<FaceSource>]> {
        self.face_sources.as_deref().map(Vec::as_slice)
    }

    /// A current wall reconstructed from an exact contiguous section run.
    /// This transient witness grants no durable source/producer identity.
    pub fn face_was_coalesced(&self, index: usize) -> bool {
        matches!(self.face_sources().and_then(|s| s.get(index)), Some(Some(FaceSource::ExtrudeSide { coalesced: true, .. })))
    }

    /// An exact construction/import plane; never inferred from tessellation.
    pub fn face_plane(&self, index: usize) -> Option<solidcraft_geom::Plane> {
        if index >= 50_000 {
            return None;
        }
        let source = self.face_sources()?.get(index)?.as_ref()?;
        match source {
            FaceSource::ExtrudeSide { plane, .. } | FaceSource::CertifiedExtrudeSide { plane, .. } => *plane,
            FaceSource::ExtrudeCap { plane, .. }
            | FaceSource::CertifiedExtrudeCap { plane, .. }
            | FaceSource::Box { plane, .. }
            | FaceSource::CylinderCap { plane, .. }
            | FaceSource::ShellPlanar { plane, .. } => Some(*plane),
            FaceSource::Imported { .. } => self.planar_face(index),
        }
    }

    /// Exact current planar surface for an explicitly fixed snapshot, without source lineage: the
    /// face's plane as the face faces (out of the material in a closed shell).
    pub fn planar_face(&self, index: usize) -> Option<solidcraft_geom::Plane> {
        let face = self.record.faces.get(index)?;
        let (o, u, v) = match &face.surface {
            Surface::Plane { origin, u_axis, v_axis } => (v3(*origin), v3(*u_axis), v3(*v_axis)),
            Surface::PlaneThrough { origin, u_point, v_point } => {
                let o = v3(*origin);
                (o, v3(*u_point) - o, v3(*v_point) - o)
            }
            _ => return None,
        };
        let v = if self.face_forward(index) { v } else { -v };
        solidcraft_geom::Plane::new(o, u, v)
    }

    /// Certify the two caps of a body built by [`crate::cylinder`]. Its wall stays uncertified.
    pub fn with_cylinder_certificate(mut self, producer: u64) -> Result<Self> {
        if producer == 0 {
            return Err(KernelError::Invalid("invalid cylinder producer".into()));
        }
        let Some(sources) = &self.face_sources else {
            return Err(KernelError::Failed("cylinder construction source".into()));
        };
        let mut sources = sources.as_ref().clone();
        let mut ends = [false; 2];
        for source in sources.iter_mut() {
            if let Some(FaceSource::ExtrudeCap { high, plane, .. }) = source {
                let end = ends.get_mut(usize::from(*high)).ok_or_else(|| KernelError::Failed("cylinder cap role".into()))?;
                if std::mem::replace(end, true) {
                    return Err(KernelError::Failed("duplicate cylinder cap role".into()));
                }
                *source = Some(FaceSource::CylinderCap { producer, high: *high, plane: *plane });
            }
        }
        if ends != [true; 2] {
            return Err(KernelError::Failed("cylinder cap role count".into()));
        }
        self.face_sources = Some(std::sync::Arc::new(sources));
        Ok(self)
    }

    pub fn with_box_certificate(mut self, producer: u64) -> Result<Self> {
        if producer == 0 {
            return Err(KernelError::Invalid("invalid box producer".into()));
        }
        if let Some(sources) = &self.face_sources {
            let mut sources = sources.as_ref().clone();
            for source in sources.iter_mut().flatten() {
                if let FaceSource::Box { producer: id, .. } = source {
                    *id = Some(producer);
                }
            }
            self.face_sources = Some(std::sync::Arc::new(sources));
        }
        Ok(self)
    }

    /// Cap signatures are in face order; unknown or ambiguous source curves stay uncertified.
    pub fn with_extrude_cap_certificates(mut self, producer: u64, sketch: u64, signatures: &[Option<CapSignature>]) -> Result<Self> {
        if producer == 0 || sketch == 0 {
            return Err(KernelError::Invalid("invalid cap producer".into()));
        }
        if let Some(sources) = &self.face_sources {
            if sources.len() != signatures.len() {
                return Err(KernelError::Invalid("cap certificate count mismatch".into()));
            }
            let mut sources = sources.as_ref().clone();
            for (source, signature) in sources.iter_mut().zip(signatures) {
                if let Some(FaceSource::ExtrudeCap { high, plane, .. }) = source {
                    let Some((outer, holes)) = signature else {
                        *source = None;
                        continue;
                    };
                    if outer.len() > 4096 || holes.len() > 4096 || holes.iter().any(|hole| hole.len() > 4096) {
                        return Err(KernelError::Invalid("oversized cap curve signature".into()));
                    }
                    let count = outer.len() + holes.iter().map(Vec::len).sum::<usize>();
                    if outer.is_empty()
                        || count > 4096
                        || holes.len() > 4096
                        || outer.iter().chain(holes.iter().flatten()).map(String::len).sum::<usize>() > 65_536
                        || outer.iter().chain(holes.iter().flatten()).any(|id| id.is_empty() || id.len() > 128)
                    {
                        return Err(KernelError::Invalid("invalid cap curve signature".into()));
                    }
                    let mut outer = outer.clone();
                    outer.sort();
                    outer.dedup();
                    let mut holes = holes.clone();
                    for hole in &mut holes {
                        if hole.is_empty() {
                            return Err(KernelError::Invalid("empty cap hole signature".into()));
                        }
                        hole.sort();
                        hole.dedup();
                    }
                    holes.sort();
                    if holes.windows(2).any(|pair| pair[0] == pair[1]) || holes.iter().flatten().any(|id| outer.binary_search(id).is_ok()) {
                        return Err(KernelError::Invalid("ambiguous cap hole signature".into()));
                    }
                    *source = Some(FaceSource::CertifiedExtrudeCap { producer, sketch, high: *high, outer, holes, plane: *plane });
                }
            }
            self.face_sources = Some(std::sync::Arc::new(sources));
        }
        Ok(self)
    }

    pub(crate) fn transformed_sources(&self, m: [[f64; 4]; 4]) -> Result<Option<FaceSources>> {
        let Some(sources) = &self.face_sources else { return Ok(None) };
        let vector = |p: Vec3| {
            Vec3::new(
                m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z,
                m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z,
                m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z,
            )
        };
        let translation = Vec3::new(m[3][0], m[3][1], m[3][2]);
        let reflected = vector(Vec3::X).dot(vector(Vec3::Y).cross(vector(Vec3::Z))) < 0.;
        let mut sources = sources.as_ref().clone();
        for source in sources.iter_mut().flatten() {
            if let Some(plane) = source.plane_mut() {
                *plane = solidcraft_geom::Plane::new(
                    vector(plane.origin) + translation,
                    vector(plane.x),
                    vector(plane.y) * if reflected { -1. } else { 1. },
                )
                .ok_or_else(|| KernelError::Invalid("degenerate transformed face plane".into()))?;
            }
        }
        Ok(Some(std::sync::Arc::new(sources)))
    }

    /// Bind constructed edges to uniquely proven source curves without changing face names.
    pub fn with_extrude_certificates(mut self, producer: u64, sketch: u64, curves: &[Option<String>]) -> Result<Self> {
        if producer == 0 || sketch == 0 || curves.iter().flatten().any(|curve| curve.is_empty() || curve.len() > 128) {
            return Err(KernelError::Invalid("invalid constructed side certificate".into()));
        }
        if let Some(sources) = &self.face_sources {
            if sources.len() != curves.len() {
                return Err(KernelError::Invalid("side certificate count mismatch".into()));
            }
            let sources = sources
                .iter()
                .zip(curves)
                .map(|(source, curve)| match (source, curve) {
                    (Some(FaceSource::ExtrudeSide { plane, .. }), Some(curve)) => {
                        Some(FaceSource::CertifiedExtrudeSide { producer, sketch, curve: curve.clone(), plane: *plane })
                    }
                    (Some(FaceSource::ExtrudeSide { .. }), None) => None,
                    _ => source.clone(),
                })
                .collect();
            self.face_sources = Some(std::sync::Arc::new(sources));
        }
        Ok(self)
    }

    pub fn with_import_certificate(mut self, digest: &str, producer: u64) -> Result<Self> {
        if producer == 0 || digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
            return Err(KernelError::Invalid("invalid immutable import certificate".into()));
        }
        if let Some(sources) = &self.face_sources {
            let mut sources = sources.as_ref().clone();
            for source in sources.iter_mut().flatten() {
                if let FaceSource::Imported { payload_sha256, producer: origin, .. } = source {
                    *payload_sha256 = Some(digest.into());
                    *origin = Some(producer);
                }
            }
            self.face_sources = Some(std::sync::Arc::new(sources));
        }
        Ok(self)
    }

    pub(crate) fn with_face_sources(mut self, sources: Option<std::sync::Arc<Vec<Option<FaceSource>>>>) -> Self {
        self.face_sources = sources;
        self
    }

    /// Is this a mesh body (no B-rep)?
    pub fn is_mesh(&self) -> bool {
        self.mesh.is_some()
    }

    /// Error for operations that need a B-rep.
    pub(crate) fn require_brep(&self, what: &str) -> Result<()> {
        if self.is_mesh() {
            Err(KernelError::Invalid(format!("{what} needs a solid body; mesh bodies can only be moved, measured and exported")))
        } else {
            Ok(())
        }
    }

    /// Display colour from an imported file.
    pub fn color(&self) -> Option<[f32; 3]> {
        self.color
    }

    /// The same body, with how far its boundaries may lie off their surfaces (mm) as the file it
    /// was read from declares (see [`Body::boundary_tol`]). A value that is not finite and above
    /// zero clears the declaration, which puts the body back on the kernel's own bound.
    pub(crate) fn with_boundary_tol(mut self, tol: Option<f64>) -> Body {
        self.boundary_tol = tol.filter(|t| t.is_finite() && *t > 0.0);
        self
    }

    /// The same body with a display colour.
    pub fn with_color(mut self, c: Option<[f32; 3]>) -> Body {
        self.color = c.filter(|c| c.iter().all(|x| x.is_finite())).map(|c| c.map(|x| x.clamp(0.0, 1.0)));
        self
    }

    /// Lines faces were split along on purpose (Split Face), as polylines.
    pub fn split_lines(&self) -> Option<&std::sync::Arc<Vec<Vec<Vec3>>>> {
        self.splits.as_ref()
    }

    pub(crate) fn split_keep(&self) -> &[Vec<Vec3>] {
        self.splits.as_ref().map(|l| l.as_slice()).unwrap_or(&[])
    }

    /// The same body with the lines its faces were split along on purpose.
    /// This body shows `shared`'s mesh placed by `t` (it is that body's solid, placed by `t`).
    pub(crate) fn with_display_of(mut self, shared: std::sync::Arc<SharedDisplay>, t: [[f64; 4]; 4]) -> Body {
        if self.splits.is_none() && !self.is_mesh() {
            self.display_of = Some(std::sync::Arc::new((shared, t)));
        }
        self
    }

    /// The display this body shows placed by a matrix (by columns), when it is a placed copy of
    /// another body.
    pub fn display_share(&self) -> Option<(DisplayShare, [[f64; 4]; 4])> {
        self.display_of.as_deref().map(|(shared, t)| (DisplayShare(shared.clone()), *t))
    }

    /// The same body showing `share`'s display placed by `t` (see [`Body::display_share`]).
    pub fn with_display_share(self, share: &DisplayShare, t: [[f64; 4]; 4]) -> Body {
        self.with_display_of(share.0.clone(), t)
    }

    /// Identity of the solid: clones of a body (and bodies a feature passed through unchanged)
    /// share it.
    pub fn solid_id(&self) -> usize {
        std::sync::Arc::as_ptr(&self.record) as usize
    }

    pub fn with_split_lines(mut self, s: Option<std::sync::Arc<Vec<Vec<Vec3>>>>) -> Body {
        self.splits = s.filter(|l| !l.is_empty());
        // Split lines are drawn: the shared mesh does not have them.
        if self.splits.is_some() {
            self.display_of = None;
        }
        self.regularization_protected |= self.splits.is_some();
        self
    }

    /// Opacity and face colours for export, if any.
    pub fn paint(&self) -> Option<&Paint> {
        self.paint.as_deref()
    }

    /// The same body with opacity and face colours for export (`None` or opaque with no face
    /// colours clears them).
    pub fn with_paint(mut self, p: Option<Paint>) -> Body {
        let clean = |x: f32| if x.is_finite() { x.clamp(0.0, 1.0) } else { 1.0 };
        self.paint = p
            .map(|mut p| {
                p.opacity = clean(p.opacity);
                p.faces.retain(|f| f.color.iter().all(|x| x.is_finite()));
                for f in &mut p.faces {
                    f.color = f.color.map(|x| x.clamp(0.0, 1.0));
                    f.opacity = clean(f.opacity);
                }
                p
            })
            .filter(|p| p.opacity < 1.0 || !p.faces.is_empty())
            .map(std::sync::Arc::new);
        self.regularization_protected |= self.paint().is_some_and(|paint| !paint.faces.is_empty());
        self
    }

    /// Characteristic size (bounding box diagonal of its vertices) for tolerances, measured
    /// when the body was made; mesh bodies measure their triangles.
    pub fn size(&self) -> f64 {
        if self.size > 0.0 {
            return self.size;
        }
        let mut b = Aabb3::EMPTY;
        if let Some(m) = &self.mesh {
            for p in &m.positions {
                b.add(*p);
            }
        }
        for v in &self.record.vertices {
            b.add(v3(v.point));
        }
        let d = b.diagonal();
        if d.is_finite() && d > 0.0 { d } else { 1.0 }
    }

    /// A box around the body worked out without meshing it: its vertices and its edges' control
    /// points (a spline lies in the hull of its controls; a face's surface does not count, as an
    /// imported face is often trimmed from a far larger one). A face curving out beyond all of
    /// its edges (a dome on a circle) can reach past it, so it frames a view while the body is
    /// still being meshed; [`Body::exact_bounds`] is the exact one. A mesh body's box is its
    /// triangles'.
    pub fn outline_bounds(&self) -> Aabb3 {
        let mut b = Aabb3::EMPTY;
        if let Some(m) = &self.mesh {
            for p in &m.positions {
                b.add(*p);
            }
            return b;
        }
        for v in &self.record.vertices {
            b.add(v3(v.point));
        }
        for e in &self.record.edges {
            curve_hull(&e.curve, e.interval, &mut b);
        }
        b
    }

    /// The exact box of the body's faces, edges and vertices (SMLib's tight box); a mesh body's
    /// is its triangles'.
    pub fn exact_bounds(&self) -> Result<Aabb3> {
        if self.is_mesh() {
            return Ok(self.outline_bounds());
        }
        let (lo, hi) = self.smlib()?.bounds().map_err(|e| KernelError::Failed(format!("SMLib bounds: {e}")))?;
        Ok(Aabb3 { min: v3(lo), max: v3(hi) })
    }

    /// The body's volume from a mesh at `tol`, measured once per (record, tolerance): clones of
    /// a body share the measurement. `None` when the body cannot be meshed.
    pub fn volume_at(&self, tol: f64) -> Option<f64> {
        let key = if tol.is_finite() && tol > 0.0 { tol } else { 0.05 };
        let key = key.to_bits();
        if let Some(cell) = &self.volume
            && let Ok(cached) = cell.lock()
            && let Some((cached_tol, volume)) = *cached
            && cached_tol == key
        {
            return Some(volume);
        }
        let volume = self.mesh(&MeshTolerance::chord(f64::from_bits(key))).ok()?.measure().volume;
        if let Some(cell) = &self.volume
            && let Ok(mut slot) = cell.lock()
        {
            *slot = Some((key, volume));
        }
        Some(volume)
    }

    /// Rough heap footprint, for the evaluation cache's byte budget: the B-rep topology is what
    /// makes a kept timeline snapshot expensive, so it dominates the estimate.
    pub fn retained_bytes(&self) -> usize {
        if let Some(m) = &self.mesh {
            return m.positions.len() * 24 + m.triangles.len() * 12;
        }
        let uses: usize = self.record.faces.iter().flat_map(|f| &f.loops).map(|l| if let Loop::Edges { edges } = l { edges.len() } else { 1 }).sum();
        self.record.faces.len() * 2048 + uses * (512 + 96)
    }

    /// The body's triangles, `t` from its exact surfaces: SolidCraft's one mesher (the planner,
    /// [`crate::tess`]), evaluated on the CPU, with its edges flagged for display (seams, tangent
    /// joins, protected creases). Triangles carry their face index and the edges their polylines,
    /// in the record's order; neighbouring faces share every boundary node, so the mesh is
    /// watertight wherever the B-rep is. A mesh body gives its own triangles. A face the planner
    /// cannot mesh is an error that names it and why.
    pub fn mesh(&self, t: &MeshTolerance) -> Result<Mesh> {
        crate::profile::timed(crate::profile::Kind::Tessellate, || self.mesh_untimed(t))
    }

    fn mesh_untimed(&self, t: &MeshTolerance) -> Result<Mesh> {
        if let Some(m) = &self.mesh {
            return Ok(m.to_mesh());
        }
        // A placed copy shows its original's mesh, moved: the part is meshed once for every copy.
        if let Some((shared, m)) = self.display_of.as_deref() {
            let made = remembered(&shared.meshes, t, || shared.original.mesh_untimed(t))?;
            if let Some(placed) = placed_mesh(&made, m) {
                return Ok(placed);
            }
        }
        match self.meshes.as_ref().filter(|m| std::sync::Arc::ptr_eq(&m.record, &self.record)) {
            Some(memo) => Ok(Mesh::clone(&remembered(&memo.made, t, || self.mesh_of(&self.mesh_plan_untimed(t)?))?)),
            None => self.mesh_of(&self.mesh_plan_untimed(t)?),
        }
    }

    /// The plan [`Body::mesh`] evaluates: every face's triangulation in its own parameters, each
    /// edge sampled once so faces meet exactly. For an evaluator of its own (the GPU viewport);
    /// [`Body::mesh_of`] gives its mesh without planning again.
    pub fn mesh_plan(&self, t: &MeshTolerance) -> Result<solidcraft_geom::tess::TessPlan> {
        crate::profile::timed(crate::profile::Kind::Tessellate, || self.mesh_plan_untimed(t))
    }

    fn mesh_plan_untimed(&self, t: &MeshTolerance) -> Result<solidcraft_geom::tess::TessPlan> {
        if self.is_mesh() {
            return Err(KernelError::Invalid("a mesh body has no surfaces to mesh".into()));
        }
        let (tol, turn) = t.rules(self.size());
        guard("mesh", || crate::tess::plan(&self.record, tol, turn).map(|(plan, _)| plan))
    }

    /// The mesh of `plan`, a plan [`Body::mesh_plan`] made of this body: what [`Body::mesh`]
    /// gives at the plan's tolerance.
    pub fn mesh_of(&self, plan: &solidcraft_geom::tess::TessPlan) -> Result<Mesh> {
        self.flagged(plan.evaluate())
    }

    /// A copy of this body that keeps the meshes made of it for reuse, for as long as the copy
    /// (or a clone of it) lives: for many operations on one large body at the same tolerance,
    /// such as cutting it down to the region of each of many contacts.
    pub fn keeping_meshes(&self) -> Body {
        Body { meshes: Some(std::sync::Arc::new(MeshMemo { record: self.record.clone(), made: std::sync::Mutex::new(Vec::new()) })), ..self.clone() }
    }

    /// Planar faces whose boundary crosses itself: two of its loops crossing each other, or a
    /// loop crossing itself. Such a face has no inside, and its mesh covers what it shouldn't
    /// and leaves openings beside it, while the shell is still closed and every face meshes
    /// (rounds that overlap where a wall is thinner than both, #871). Boundaries that only
    /// touch (at a vertex they share, or within the size's millionth) don't count.
    pub fn crossed_face_loops(&self) -> Result<usize> {
        Ok(self.crossed_faces()?.len())
    }

    /// The faces [`Body::crossed_face_loops`] counts, each as the mean of its boundary's
    /// points: the same for a face an operation carried over unchanged.
    pub fn crossed_faces(&self) -> Result<Vec<Vec3>> {
        if self.is_mesh() {
            return Ok(Vec::new());
        }
        const MAX_SEGMENTS: usize = 20_000;
        let size = self.size();
        let tol = (size * 1e-6).max(1e-9);
        let chord = (size * 1e-3).max(1e-4);
        let mut crossed = Vec::new();
        for (fi, f) in self.record.faces.iter().enumerate() {
            let Some(plane) = self.planar_face(fi) else { continue };
            let (o, u, v) = (plane.origin, plane.x, plane.y);
            let flat = |p: Vec3| {
                let d = p - o;
                (d.dot(u), d.dot(v))
            };
            // Each loop's segments, in the face's plane.
            let mut loops: Vec<Vec<((f64, f64), (f64, f64))>> = Vec::new();
            let mut total = 0usize;
            for l in &f.loops {
                let Loop::Edges { edges } = l else { continue };
                let mut segs = Vec::new();
                for u in edges {
                    let e = self.record.edges.get(u.edge as usize).ok_or_else(|| KernelError::Invalid("edge reference".into()))?;
                    let pts = edge_points(e, chord)?;
                    segs.extend(pts.windows(2).filter_map(|q| Some((flat(*q.first()?), flat(*q.get(1)?)))));
                }
                total = total.saturating_add(segs.len());
                if total > MAX_SEGMENTS {
                    break;
                }
                loops.push(segs);
            }
            if total > MAX_SEGMENTS {
                continue;
            }
            // Proper crossings only: each segment's ends strictly on either side of the other.
            let side = |a: (f64, f64), b: (f64, f64), p: (f64, f64)| {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let len = (dx * dx + dy * dy).sqrt();
                if len <= tol { 0.0 } else { (dx * (p.1 - a.1) - dy * (p.0 - a.0)) / len }
            };
            let cross = |s: &((f64, f64), (f64, f64)), t: &((f64, f64), (f64, f64))| {
                let (d1, d2) = (side(s.0, s.1, t.0), side(s.0, s.1, t.1));
                let (d3, d4) = (side(t.0, t.1, s.0), side(t.0, t.1, s.1));
                ((d1 > tol && d2 < -tol) || (d1 < -tol && d2 > tol)) && ((d3 > tol && d4 < -tol) || (d3 < -tol && d4 > tol))
            };
            let all: Vec<&((f64, f64), (f64, f64))> = loops.iter().flatten().collect();
            let bad = all.iter().enumerate().any(|(i, s)| all.iter().skip(i + 1).any(|t| cross(s, t)));
            if bad {
                let k = all.len().max(1) as f64;
                let (su, sv) = all.iter().fold((0.0, 0.0), |acc, s| (acc.0 + s.0.0, acc.1 + s.0.1));
                crossed.push(o + u * (su / k) + v * (sv / k));
            }
        }
        Ok(crossed)
    }

    /// Faces with no triangles in a coarse mesh (the mesher failed or gave up).
    pub fn unmeshed_faces(&self) -> usize {
        if self.is_mesh() {
            return 0;
        }
        let Ok(m) = self.mesh(&MeshTolerance::chord(coarse_tolerance(self.size()))) else { return self.face_count() };
        let mut has = vec![false; self.face_count()];
        for f in &plan.faces {
            if !f.triangles.is_empty()
                && let Some(h) = has.get_mut(f.face as usize)
            {
                *h = true;
            }
        }
        Some(has.iter().filter(|h| !**h).count())
    }

    /// A mesh with its edges flagged for display (seams, tangent joins, protected creases).
    fn flagged(&self, mut m: Mesh) -> Result<Mesh> {
        if self.is_mesh() {
            return Ok(m);
        }
        m.seams = crate::topo::seam_flags(self, &m);
        (m.smooth_edges, m.sharp_edges) = crate::smooth_edges::flags(self, &m);
        Ok(m)
    }

    /// Edges with polylines (index = the record's edge index).
    pub fn edges(&self, tol: f64) -> Result<Vec<EdgeInfo>> {
        let m = self.mesh(&MeshTolerance::chord(tol))?;
        Ok(m.edges
            .into_iter()
            .enumerate()
            .map(|(index, points)| {
                let (mid, length) = polyline_mid(&points);
                EdgeInfo { index, points, mid, length }
            })
            .collect())
    }

    /// Edges `indices` (record edge indices) in `plane`'s coordinates, exactly: lines, circular
    /// arcs, and free-form curves as the Bézier spans they are, for a sketch to project them.
    /// `None` for an edge that doesn't lie in a plane parallel to `plane`, isn't read exactly or
    /// isn't there, and for every edge of a mesh body.
    pub fn planar_edges(&self, plane: &solidcraft_geom::Plane, indices: &[usize]) -> Vec<Option<Vec<solidcraft_geom::Seg2>>> {
        if self.is_mesh() {
            return vec![None; indices.len()];
        }
        let tol = (self.size() * 1e-7).max(1e-9) * 10.0;
        indices
            .iter()
            .map(|i| {
                let mut work = 262_144usize;
                crate::prism_curve::Run::of(&self.record, *i, true).and_then(|run| crate::prism_curve::projected(&run, plane, tol, &mut work))
            })
            .collect()
    }

    /// Faces with area and centroid.
    pub fn faces(&self, tol: f64) -> Result<Vec<FaceInfo>> {
        let m = self.mesh(&MeshTolerance::chord(tol))?;
        let nf = self.face_count();
        let mut acc: Vec<(f64, Vec3, Vec3, bool)> = vec![(0.0, Vec3::ZERO, Vec3::ZERO, true); nf];
        for (t, f) in m.triangles.iter().zip(&m.tri_face) {
            let Some([a, b, c]) = m.tri(t) else { continue };
            let cr = (b - a).cross(c - a);
            let ar = cr.len() * 0.5;
            if let Some(slot) = acc.get_mut(*f as usize) {
                slot.0 += ar;
                slot.1 += (a + b + c) * (ar / 3.0);
                if let Some(n) = cr.normalized() {
                    if slot.2 == Vec3::ZERO {
                        slot.2 = n;
                    } else if slot.2.dot(n) < 1.0 - 1e-6 {
                        slot.3 = false;
                    }
                }
            }
        }
        Ok(acc
            .into_iter()
            .enumerate()
            .map(|(index, (area, c, n, planar))| FaceInfo {
                index,
                area,
                centroid: if area > 0.0 { c / area } else { Vec3::ZERO },
                plane_normal: (planar && n != Vec3::ZERO).then_some(n),
            })
            .collect())
    }

    /// Index of the edge closest to `p` (by polyline distance).
    pub fn nearest_edge(&self, p: Vec3, tol: f64) -> Result<Option<(usize, f64)>> {
        let edges = self.edges(tol)?;
        let mut best: Option<(usize, f64)> = None;
        for e in &edges {
            for w in e.points.windows(2) {
                if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
                    let d = p.dist_to_segment(*a, *b);
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((e.index, d));
                    }
                }
            }
        }
        Ok(best)
    }

    /// Is the point inside the solid?
    pub fn contains(&self, p: Vec3) -> bool {
        self.mesh(&MeshTolerance::chord(self.size() * 2e-3)).map(|m| m.contains(p)).unwrap_or(false)
    }

    /// B-rep faces (a mesh body is one face; [`crate::measure`] counts its triangles).
    pub fn face_count(&self) -> usize {
        if self.mesh.is_some() { 1 } else { self.record.faces.len() }
    }
    /// B-rep edges (none for a mesh body).
    pub fn edge_count(&self) -> usize {
        self.record.edges.len()
    }
    /// B-rep vertices (none for a mesh body).
    pub fn vertex_count(&self) -> usize {
        self.record.vertices.len()
    }

    /// What is wrong with the body as a solid: face shells that aren't closed, faces that don't
    /// mesh, a volume that isn't positive. Empty when it is sound.
    pub fn validity(&self) -> Vec<String> {
        if let Some(m) = &self.mesh {
            return if crate::meshbody::is_closed(m) { Vec::new() } else { vec!["the mesh is not closed".into()] };
        }
        let mut out = Vec::new();
        for (i, shell) in self.record.shells.iter().enumerate() {
            if let Shell::Faces { .. } = shell
                && !self.record.regions.iter().skip(1).any(|r| r.shells.first() == Some(&(i as u32)))
            {
                out.push(format!("shell {i} is open, not closed"));
            }
        }
        let missing = self.unmeshed_faces();
        if missing > 0 {
            out.push(format!("{missing} face(s) don't mesh"));
        }
        match crate::measure(self) {
            Ok(m) if m.volume.is_finite() && m.volume > 0.0 => {}
            Ok(m) => out.push(format!("volume {}", m.volume)),
            Err(e) => out.push(format!("no measure: {e}")),
        }
        out
    }

    /// Face shells (a mesh body is one).
    pub fn shell_count(&self) -> usize {
        if self.mesh.is_some() { 1 } else { self.record.shells.iter().filter(|s| matches!(s, Shell::Faces { .. })).count() }
    }

    /// The separate pieces of a body: each material region with the voids inside it, read from
    /// the record's regions. A body in one piece comes back as itself. Pieces of a body in
    /// several carry their history over `[self]` (their faces and edges are its own), so
    /// whoever names them can follow it.
    pub fn lumps(&self) -> Result<Vec<Body>> {
        let regions = self.record.material_regions();
        if self.is_mesh() || regions.len() < 2 {
            return Ok(vec![self.clone()]);
        }
        let counts = [(self.face_count(), self.edge_count())];
        regions
            .into_iter()
            .map(|r| {
                let part = self.record.part(&[r]).map_err(|e| KernelError::Failed(format!("lump: {e}")))?;
                let sources = self.face_sources.as_ref().map(|all| {
                    std::sync::Arc::new(part.faces.iter().map(|&f| all.get(f as usize).cloned().flatten()).collect::<Vec<_>>())
                });
                let history = crate::history::History::of_part(&part);
                let piece = Body::from_record(part.brep)?.with_face_sources(sources).inherit_regularization_protection(self);
                Ok(piece.with_checked_history(Some(history), &counts))
            })
            .collect()
    }

    /// Whether this body and `other` share material or touch (within a fine mesh tolerance).
    pub fn meets(&self, other: &Body) -> Result<bool> {
        self.require_brep("contact test")?;
        other.require_brep("contact test")?;
        let tol = (self.size().max(other.size()) * 1e-4).max(1e-6);
        let (a, b) = (self.mesh(&MeshTolerance::chord(tol))?, other.mesh(&MeshTolerance::chord(tol))?);
        shell_groups::meshes_meet(&a, &b, 2.0 * tol)
    }

    /// The same contact test for several tools, reusing this unchanged body's
    /// fine mesh at each required tolerance. No contact threshold is relaxed.
    pub fn meets_each(&self, others: &[Body]) -> Result<Vec<bool>> {
        if others.len() > 1024 {
            return Err(KernelError::Invalid("too many contact tools".into()));
        }
        self.require_brep("contact test")?;
        let size = self.size();
        let mut meshes = std::collections::HashMap::new();
        let mut contacts = Vec::with_capacity(others.len());
        for other in others {
            other.require_brep("contact test")?;
            let tol = (size.max(other.size()) * 1e-4).max(1e-6);
            let key = tol.to_bits();
            if !meshes.contains_key(&key) {
                // Bound retained meshes even when hostile inputs request many tolerances.
                if meshes.len() == 16 {
                    meshes.clear();
                }
                meshes.insert(key, self.mesh(&MeshTolerance::chord(tol))?);
            }
            let mesh = meshes.get(&key).ok_or_else(|| KernelError::Internal("contact mesh disappeared".into()))?;
            contacts.push(shell_groups::meshes_meet(mesh, &other.mesh(&MeshTolerance::chord(tol))?, 2.0 * tol)?);
        }
        Ok(contacts)
    }

    /// For each shell: whether it bounds material (else a void or an open/wire shell), and the
    /// shell directly around it (the outer shell of the region it lies in; `None` in the open).
    pub(crate) fn shell_nesting(&self) -> (Vec<bool>, Vec<Option<usize>>) {
        let n = self.record.shells.len();
        let (mut material, mut parent) = (vec![false; n], vec![None; n]);
        for (r, region) in self.record.regions.iter().enumerate() {
            let around = if r == 0 { None } else { region.shells.first().map(|&s| s as usize) };
            for (k, &s) in region.shells.iter().enumerate() {
                let s = s as usize;
                if r > 0 && k == 0 {
                    if let Some(m) = material.get_mut(s) {
                        *m = !region.void;
                    }
                } else if let Some(p) = parent.get_mut(s) {
                    *p = around;
                }
            }
        }
        (material, parent)
    }
}

/// Points of an edge's curve over its interval, within `chord` of it: lines by their ends, the
/// rest at the evaluator's points (capped).
fn edge_points(e: &brep::Edge, chord: f64) -> Result<Vec<Vec3>> {
    const MAX_POINTS: usize = 4096;
    let curve = brep::eval::CurveEval::new(&e.curve).map_err(|err| KernelError::Invalid(err.to_string()))?;
    let [t0, t1] = e.interval;
    let at = |t: f64| curve.point(t).ok_or_else(|| KernelError::Invalid(format!("edge curve has no point at {t}")));
    if let Curve::Line { .. } = e.curve {
        return Ok(vec![at(t0)?, at(t1)?]);
    }
    // Halve until every chord's midpoint is within `chord` of the curve.
    let mut ts = vec![t0, t1];
    let mut k = 0;
    while k + 1 < ts.len() && ts.len() < MAX_POINTS {
        let (a, b) = (ts[k], ts[k + 1]);
        let m = 0.5 * (a + b);
        let (pa, pb, pm) = (at(a)?, at(b)?, at(m)?);
        if pm.dist(pa.lerp(pb, 0.5)) > chord {
            ts.insert(k + 1, m);
        } else {
            k += 1;
        }
    }
    ts.into_iter().map(at).collect()
}

/// Grow `b` by a curve's hull over `interval`: its control points for splines, its points for
/// lines, circles and ellipses (their exact extreme points), the leader's hull for an
/// intersection curve.
fn curve_hull(c: &Curve, interval: [f64; 2], b: &mut Aabb3) {
    match c {
        Curve::Line { .. } | Curve::Circle { .. } | Curve::Ellipse { .. } => {
            if let Ok(e) = brep::eval::CurveEval::new(c) {
                let [t0, t1] = interval;
                for i in 0..=16 {
                    if let Some(p) = e.point(t0 + (t1 - t0) * f64::from(i) / 16.0) {
                        b.add(p);
                    }
                }
            }
        }
        Curve::Polyline { points } | Curve::Spline { points, .. } => points.iter().for_each(|p| b.add(v3(*p))),
        Curve::RationalSpline { homogeneous, .. } => {
            for h in homogeneous {
                if h[3] > 0.0 {
                    b.add(Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3]));
                }
            }
        }
        Curve::SurfaceIntersection { leader, .. } => curve_hull(leader, interval, b),
    }
}

#[cfg(test)]
#[path = "body_lump_tests.rs"]
mod lump_tests;

#[cfg(test)]
#[path = "mesh_memo_tests.rs"]
mod mesh_memo_tests;

#[cfg(test)]
#[path = "display_mesh_tests.rs"]
mod display_mesh_tests;

#[cfg(test)]
#[path = "regularization_protection_tests.rs"]
mod regularization_protection_tests;
