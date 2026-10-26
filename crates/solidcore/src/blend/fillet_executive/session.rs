//! Live object storage for one fillet executive.
//!
//! Upstream: `source/SMLib/src/SmFilletExecutive.cpp`, `source/SMLib/inc/SmFilletExecutive.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`FilletSession`] | `SmFilletExecutive` and its owned objects |
//! | [`SolverObject`] | the concrete `SmFilletSolver` objects |
//! | [`FilletSession::set_self_intersection_handler`] | `SetSelfIntersectionHandler` |
//! | [`FilletSession::fillet_corners`] | `GetFilletCorners` |
//! | [`FilletSession::set_bevel_corner`] | `SetBevelCorner` |
//! | [`FilletSession::prepare_variable_radius_edges`] | `SetFilletParameters` (same parameters on each edge) with `SM_FS_VARIABLE_RADIUS` |
//!
//! Deviations: the retained handler's algorithm type is copied into every loaded
//! solver (on load and on each set), standing in for the solvers' live
//! `m_pExecutive->m_pSelfIntersectionHandler` read. One borrowed target and typed owned slots replace native pointers.
//! Slots are never reused. Target curves cannot be replaced while its loading caches
//! are retained: the session exposes only shared access to its borrowed target.
//! Loading creates and retains default full-tree curve caches: only their knot
//! continuities are consumed, calculated at a fixed 0.5 degrees before tessellation
//! (`SmCurveCache.cpp:1508`, `curve_cache.rs:608`). The frozen target makes repeated
//! loads equivalent. An external custom/non-full-tree cache provider is not yet
//! accepted; this is not an arbitrary warmed-cache parity claim.
//! Live corner stages dispatch `NByOne` and `NByTwo` corners to the ported corner
//! code; an Nx2 corner's vertex calculators receive every retained solver of the
//! vertex's geometries (one indexed table upstream). Two geometries on one solver at a
//! rail/rail vertex are an input error here (the indexed path runs on a copy).
//! A caller's fillet law is retained once and each variable-radius solver gets its
//! own copy (upstream every solver of one `SetFilletParameters` call points at the
//! caller's single law). The law's mutable state is the arc length (written, never
//! read), the B-spline extension cache (a pure function of the law curve) and
//! `SmTemporaryRadiusChange`, which only the solver it is applied to evaluates before
//! the change is reverted, so the copies evaluate as the shared object does.
//! This foundation supports solver construction/loading; it does not substitute a
//! result for the remaining fillet execution stages. No recursion.

pub mod insertion;

use std::collections::BTreeMap;

use super::p1::{ExecutiveObjects, SolverLoadingAccess};
use super::p2::{EdgeMapAccess, FilletParameters, FilletParametersAccess, SolverSpec};
use super::p3::{
    BrepCreationAccess, CornerAdjustmentAccess, CornerEdgeAccess, CornerVertexAccess, CornerView,
    ErrorInfo, FaceConstructionAccess, FaceShrinkAccess, FaceStitchAccess, FilletExecutive,
    RailClassificationAccess, RailTrimAccess, SolverGeometryAccess, StitchAttempt, VertexEntry,
};
use crate::blend::fillet_corner::p1::LawIntervalSolvers;
use crate::blend::fillet_corner::p3::{
    CornerFilletBrep, CornerPatchEnv, CornerSolverStore, FilletCorner, FilletCornerKind,
    Nx2AdjustContext,
};
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilletBrep, FilletCornerId, FilletCornerType, FilletSolverId, FilletVertexType,
};
use crate::blend::fillet_geom::p5::FilletEdgeContext;
use crate::blend::fillet_solver::p1::{
    FilletSolver, FilletSolverOps, FilletSurfaceGenerator, MakeSurfaceBlendSih,
};
use crate::blend::fillet_solver::p4::FilletGeometryContext;
use crate::blend::fillet_standard_solver::p1::{
    constant_radius_from_edgeuse, ConstantDistanceFs, ConstantRadiusFs, FilletLaw,
    FilletSolverType, FilletSurfaceGeneratorType, SurfaceSurfaceFs, VariableRadiusFs,
};
use crate::curve::curve_cache::CurveCache;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::{ids, Attribute, AttributeHandle, AttributeStore, Behavior};
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::intersect::point_class::{ClassObject, ClassObjectKind};
use crate::kernel::Kernel;
use crate::surface::surface::Surface;
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, VertexId};

mod classification;
mod finish;
mod original_merge;
mod original_trim;
pub use original_trim::{OriginalSurfaceDeletion, OriginalSurfaceSelection};

const OP: &str = "blend::fillet_executive::session";

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

pub use crate::blend::fillet_classification::FilletBody;

/// A reference to an actual object in a single executive's object store.
///
/// These identities are session-local. Bare solver/corner IDs and object values from
/// different sessions must not be mixed; they do not encode global provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FilletObject {
    /// The session's one actual executive.
    Executive,
    /// One of its actual B-reps.
    Body(FilletBody),
    /// Topology in that body's typed arena (different bodies never alias).
    Topology(FilletBody, EntityRef),
    /// The retained curve of an edge; separate from its topology identity.
    Curve(FilletBody, EdgeId),
    /// Actual curve retained by an intermediate edge.
    IntermediateCurve(EdgeId),
    /// An actual operation-owned curve copy with checked issued identity.
    InsertionCurve(insertion::curves::CurveBinding),
    /// Actual trim curve retained by an edge use.
    UvCurve(FilletBody, EdgeuseId),
    /// Actual intermediate edge-use trim curve.
    IntermediateUvCurve(EdgeuseId),
    /// Original Surface instance with checked body and geometry identity.
    Surface(insertion::geometry::SurfaceBinding),
    /// The current actual Surface of a body-qualified Face getter.
    FaceSurface(insertion::geometry::InsertionBody, FaceId),
    /// The actual intermediate B-rep allocated during face construction.
    IntermediateBrep,
    /// Actual topology in the separately allocated intermediate body.
    IntermediateTopology(EntityRef),
    /// A geometry retained in the pseudo-B-rep.
    Geometry(crate::blend::fillet_geom::p1::FilletGeomId),
    /// A concrete solver in the session's solver slots.
    Solver(FilletSolverId),
    /// A concrete corner in the session's corner slots.
    Corner(FilletCornerId),
    /// The actual retained executive topology intersector.
    Intersector,
    /// An actual retained cross-section generator.
    Generator(u32),
    /// An attribute owned by the session's actual attribute store.
    Attribute(AttributeHandle),
    /// The session's retained self-intersection handler.
    SelfIntersectionHandler,
    /// A caller-supplied fillet law retained by the session.
    Law(u32),
}

impl ClassObject for FilletObject {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Curve(..)
            | Self::IntermediateCurve(_)
            | Self::InsertionCurve(_)
            | Self::UvCurve(..)
            | Self::IntermediateUvCurve(_) => ClassObjectKind::Curve,
            Self::Surface(_) | Self::FaceSurface(..) => ClassObjectKind::Surface,
            Self::Topology(_, entity) | Self::IntermediateTopology(entity) => match entity {
                EntityRef::Vertex(_) => ClassObjectKind::Vertex,
                EntityRef::Edge(_) => ClassObjectKind::Edge,
                EntityRef::Face(_) => ClassObjectKind::Face,
                EntityRef::Region(_) => ClassObjectKind::Region,
                EntityRef::Loop(_) => ClassObjectKind::Loop,
                EntityRef::Edgeuse(_) => ClassObjectKind::Edgeuse,
                EntityRef::Vertexuse(_) => ClassObjectKind::Vertexuse,
                EntityRef::Faceuse(_) => ClassObjectKind::Faceuse,
                EntityRef::Loopuse(_) => ClassObjectKind::Loopuse,
                EntityRef::Brep(_) | EntityRef::Shell(_) => ClassObjectKind::Other,
            },
            _ => ClassObjectKind::Other,
        }
    }
}

impl SolutionObject for FilletObject {
    fn kind(&self) -> ObjectKind {
        match self {
            Self::Curve(..)
            | Self::IntermediateCurve(_)
            | Self::InsertionCurve(_)
            | Self::UvCurve(..)
            | Self::IntermediateUvCurve(_) => ObjectKind::Curve,
            Self::Surface(_) | Self::FaceSurface(..) => ObjectKind::Surface,
            Self::Topology(_, EntityRef::Vertex(_))
            | Self::IntermediateTopology(EntityRef::Vertex(_)) => ObjectKind::Vertex,
            Self::Topology(_, EntityRef::Edge(_))
            | Self::IntermediateTopology(EntityRef::Edge(_)) => ObjectKind::Edge,
            Self::Topology(_, EntityRef::Face(_))
            | Self::IntermediateTopology(EntityRef::Face(_)) => ObjectKind::Face,
            _ => ObjectKind::Other,
        }
    }
}

/// An owned concrete solver, including its one authoritative mutable base.
#[derive(Debug)]
pub enum SolverObject {
    /// `SmConstantRadiusFS` / its assisted implementation.
    ConstantRadius(Box<ConstantRadiusFs>),
    /// `SmConstantDistanceFS`.
    ConstantDistance(Box<ConstantDistanceFs>),
    /// `SmVariableRadiusFS`, retaining its actual radius law.
    VariableRadius(Box<VariableRadiusFs>),
    /// `SmSurfaceSurfaceFS`.
    SurfaceSurface(Box<SurfaceSurfaceFs>),
}

impl SolverObject {
    /// Borrow the base of this actual concrete solver.
    #[must_use]
    pub fn base(&self) -> &FilletSolver {
        match self {
            Self::ConstantRadius(solver) => &solver.solver,
            Self::ConstantDistance(solver) => &solver.solver,
            Self::VariableRadius(solver) => &solver.curve_based.solver,
            Self::SurfaceSurface(solver) => &solver.base.solver,
        }
    }

    /// Borrow the same base for a native member write.
    pub fn base_mut(&mut self) -> &mut FilletSolver {
        match self {
            Self::ConstantRadius(solver) => &mut solver.solver,
            Self::ConstantDistance(solver) => &mut solver.solver,
            Self::VariableRadius(solver) => &mut solver.curve_based.solver,
            Self::SurfaceSurface(solver) => &mut solver.base.solver,
        }
    }
}

/// Owned objects and the target borrowed by the executive.
#[derive(Debug)]
struct SessionObjects<'target> {
    target: &'target mut Brep,
    pseudo: FilletBrep,
    intermediate: Option<Brep>,
    insertion_curves: insertion::curves::InsertionCurves,
    notification_records:
        BTreeMap<crate::geom::attribute::ObjectKey, crate::topology::topology::Topology>,
    solvers: Vec<Option<SolverObject>>,
    corners: Vec<FilletCorner>,
    generators: Vec<FilletSurfaceGenerator>,
    laws: Vec<FilletLaw>,
    attributes: AttributeStore,
    merge_attributes: Option<Vec<AttributeHandle>>,
    extended_surfaces: BTreeMap<FaceId, Surface>,
    edge_caches: BTreeMap<EdgeId, CurveCache<FilletObject>>,
    target_serial: u64,
    self_intersection_handler: Option<MakeSurfaceBlendSih>,
}

impl SessionObjects<'_> {
    fn intermediate_identity(&self, entity: EntityRef) -> Result<crate::geom::object::ObjectRef> {
        let body = self
            .intermediate
            .as_ref()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        let serial = body
            .notify_serial()
            .ok_or_else(|| invalid("intermediate body has no kernel identity"))?;
        Ok(crate::topology::brep::p3::topology_ref(serial, entity))
    }
    fn fillet_surface(
        &self,
        geom: crate::blend::fillet_geom::p1::FilletGeomId,
    ) -> Result<Option<&Surface>> {
        use crate::blend::fillet_geom::p1::FilletSurfaceStorage;
        match self.pseudo.geom(geom)?.fillet_surface.as_ref() {
            None => Ok(None),
            Some(FilletSurfaceStorage::Owned(surface)) => Ok(Some(surface)),
            Some(FilletSurfaceStorage::ManagedFace { body, face }) => {
                let actual = self.intermediate_identity(EntityRef::Brep(
                    crate::topology::BrepId::from_index(0),
                ))?;
                if *body != actual {
                    return Err(invalid("fillet surface names a foreign managed body"));
                }
                let managed = self
                    .intermediate
                    .as_ref()
                    .ok_or_else(|| invalid("managed surface body is not allocated"))?;
                Ok(Some(
                    managed
                        .face(*face)?
                        .surface
                        .as_ref()
                        .ok_or_else(|| SolidError::null_pointer(OP))?,
                ))
            }
        }
    }
    fn mapping_edge(
        &self,
        object: FilletObject,
    ) -> Result<&crate::blend::fillet_geom::p1::FilletEdge> {
        let FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edge(edge)) = object else {
            return Err(invalid("mapping edge is not actual pseudo topology"));
        };
        self.pseudo.fillet_edge(edge)
    }

    fn solver(&self, id: FilletSolverId) -> Result<&SolverObject> {
        self.solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("solver is not live in this session"))
    }

    fn solver_mut(&mut self, id: FilletSolverId) -> Result<&mut SolverObject> {
        self.solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))
    }
    fn calc_solver_geometry(
        &mut self,
        k: &Kernel,
        id: FilletSolverId,
        executive: &FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let index = id.0 as usize;
        if index >= self.solvers.len() {
            return Err(invalid("solver is not live in this session"));
        }
        let (before, rest) = self.solvers.split_at_mut(index);
        let (current, after) = rest
            .split_first_mut()
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let current = current
            .as_mut()
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let context = SessionGeometryContext {
            current: id,
            before,
            after,
            executive,
            corners: &self.corners,
        };
        match current {
            SolverObject::ConstantRadius(solver) => FilletSolver::calc_fillet_geom_with_context(
                solver.as_mut(),
                k,
                self.target,
                &mut self.pseudo,
                &context,
            ),
            // `SmVariableRadiusFS` inherits `SmFilletSolver::CalcFilletGeom`.
            SolverObject::VariableRadius(solver) => FilletSolver::calc_fillet_geom_with_context(
                solver.as_mut(),
                k,
                self.target,
                &mut self.pseudo,
                &context,
            ),
            // `SmConstantDistanceFS` inherits `SmFilletSolver::CalcFilletGeom`.
            SolverObject::ConstantDistance(solver) => FilletSolver::calc_fillet_geom_with_context(
                solver.as_mut(),
                k,
                self.target,
                &mut self.pseudo,
                &context,
            ),
            // UNPORTED(blend.fillet_executive_p3): actual derived solver operation contexts.
            SolverObject::SurfaceSurface(_) => Err(SolidError::unported(
                "live concrete CalcFilletGeom operations",
                "blend.fillet_executive_p3",
            )),
        }
    }
}

/// The retained virtual operations of a concrete solver whose live corner-vertex
/// operations are ported.
fn live_solver_ops(solver: &mut SolverObject) -> Result<&mut dyn FilletSolverOps> {
    match solver {
        SolverObject::ConstantRadius(solver) => Ok(solver.as_mut()),
        SolverObject::VariableRadius(solver) => Ok(solver.as_mut()),
        SolverObject::ConstantDistance(solver) => Ok(solver.as_mut()),
        SolverObject::SurfaceSurface(_) => {
            // UNPORTED(blend.fillet_executive_p1): retained virtual operations for other concrete solver types.
            Err(SolidError::unported(
                "SmFilletSolver::RailEdgeuseIntersect (live concrete dispatch)",
                "blend.fillet_executive_p1",
            ))
        }
    }
}

/// The retained solvers of every fillet geometry at pseudo vertex `fv`, with their
/// actual identities, for the corner-vertex calculators.
impl LawIntervalSolvers for [Option<SolverObject>] {
    fn variable_radius_mut(&mut self, id: FilletSolverId) -> Result<Option<&mut VariableRadiusFs>> {
        let solver = self
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("fillet geometry solver is not live"))?;
        // pThisFS->OffsetRadiiCanChange() (FALSE for SmConstantRadiusFS), then the
        // SM_CAST_PTR(SmVariableRadiusFS, pThisFS) that only a variable-radius solver passes.
        match solver {
            SolverObject::VariableRadius(vrfs) => {
                if !vrfs.offset_radii_can_change()? {
                    return Ok(None);
                }
                Ok(Some(vrfs))
            }
            _ => Ok(None),
        }
    }
}

/// The session's solver slots as the corner-patch builders' solver store.
struct SessionSolvers<'s>(&'s [Option<SolverObject>]);

impl CornerSolverStore for SessionSolvers<'_> {
    fn get(&self, index: usize) -> Option<&FilletSolver> {
        self.0
            .get(index)
            .and_then(Option::as_ref)
            .map(SolverObject::base)
    }
    fn len(&self) -> usize {
        self.0.len()
    }
}

fn live_vertex_solvers<'s>(
    pseudo: &FilletBrep,
    solvers: &'s mut [Option<SolverObject>],
    fv: VertexId,
) -> Result<Vec<(FilletSolverId, &'s mut dyn FilletSolverOps)>> {
    let mut ids = Vec::new();
    for geometry in pseudo.vertex_fillet_geoms(fv)? {
        let id = pseudo
            .geom(geometry)?
            .solver
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    let mut live = Vec::new();
    for (index, slot) in solvers.iter_mut().enumerate() {
        let Some(id) = ids.iter().copied().find(|id| id.0 as usize == index) else {
            continue;
        };
        let solver = slot
            .as_mut()
            .ok_or_else(|| invalid("vertex solver is not live"))?;
        live.push((id, live_solver_ops(solver)?));
    }
    if live.len() != ids.len() {
        return Err(invalid("vertex solver is not live"));
    }
    Ok(live)
}

/// The live objects `SmFilletNx2Corner::AdjustTopology` reaches.
struct SessionNx2Adjust<'a> {
    k: &'a Kernel,
    target: &'a mut Brep,
    pseudo: &'a mut FilletBrep,
    corner: FilletCornerId,
}

impl Nx2AdjustContext for SessionNx2Adjust<'_> {
    fn parts(&mut self) -> (&Kernel, &mut Brep, &mut FilletBrep, Option<FilletCornerId>) {
        (
            self.k,
            &mut *self.target,
            &mut *self.pseudo,
            Some(self.corner),
        )
    }
    fn adjust_concave_corner_topology(&mut self, _corner: &mut FilletCorner) -> Result<()> {
        // UNPORTED(blend.fillet_executive_p1): actual retained Nx2 concave adjustment context.
        Err(SolidError::unported(
            "SmFilletNx2Corner::AdjustConcaveCornerTopology (live context)",
            "blend.fillet_executive_p1",
        ))
    }
    fn test_and_convert_big_rad_fillet(
        &mut self,
        _corner: &mut FilletCorner,
        _fg: crate::blend::fillet_geom::p1::FilletGeomId,
    ) -> Result<bool> {
        // UNPORTED(blend.fillet_executive_p1): actual retained big-radius conversion context.
        Err(SolidError::unported(
            "SmFilletExecutive::TestAndConvertBigRadFillet (live context)",
            "blend.fillet_executive_p1",
        ))
    }
}

struct SessionGeometryContext<'a> {
    current: FilletSolverId,
    before: &'a [Option<SolverObject>],
    after: &'a [Option<SolverObject>],
    executive: &'a FilletExecutive<FilletObject>,
    corners: &'a [FilletCorner],
}

impl SessionGeometryContext<'_> {
    fn corner(&self, vertex: Option<VertexId>) -> Result<&FilletCorner> {
        let key =
            vertex.map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(id)));
        let Some(VertexEntry::Corner(FilletObject::Corner(id))) =
            self.executive.vertex_corner_map.get(&key)
        else {
            return Err(invalid("endpoint has no completed actual corner"));
        };
        self.corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live in this session"))
    }
}

impl FilletGeometryContext for SessionGeometryContext<'_> {
    fn set_extension_flags(&self, solver: &mut dyn FilletSolverOps, target: &Brep) -> Result<()> {
        solver
            .solver_mut()
            .set_extension_flags_with_lookup(target, |vertex| self.corner(vertex))
    }

    fn solver<'s>(
        &'s self,
        id: FilletSolverId,
        current: &'s FilletSolver,
    ) -> Result<&'s FilletSolver> {
        if id == self.current {
            return Ok(current);
        }
        let index = id.0 as usize;
        let slot = if index < self.current.0 as usize {
            self.before.get(index)
        } else {
            (self.current.0 as usize)
                .checked_add(1)
                .and_then(|first| index.checked_sub(first))
                .and_then(|index| self.after.get(index))
        };
        slot.and_then(Option::as_ref)
            .map(SolverObject::base)
            .ok_or_else(|| invalid("peer solver is not live in this session"))
    }
}

struct SessionEdgeLookup<'a> {
    solvers: &'a [Option<SolverObject>],
    extended_surfaces: &'a BTreeMap<FaceId, Surface>,
}

impl FilletEdgeContext for SessionEdgeLookup<'_> {
    fn solver(&self, id: FilletSolverId) -> Result<&FilletSolver> {
        self.solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .map(SolverObject::base)
            .ok_or_else(|| invalid("solver is not live in this session"))
    }
    fn extended_surface(&self, _corner: &FilletCorner, face: FaceId) -> Result<Option<&Surface>> {
        Ok(self.extended_surfaces.get(&face))
    }
}

struct SessionCornerEdgeContext<'a> {
    k: &'a Kernel,
    target: &'a mut Brep,
    pseudo: &'a mut FilletBrep,
    lookup: SessionEdgeLookup<'a>,
    corner: FilletCornerId,
    do_global_merge: &'a bool,
}

/// One actual executive and its live target, pseudo-B-rep, solvers and corners.
#[derive(Debug)]
pub struct FilletSession<'target> {
    executive: FilletExecutive<FilletObject>,
    objects: SessionObjects<'target>,
}

impl<'target> FilletSession<'target> {
    /// Construct the native executive-owned objects, with its ordinary defaults.
    ///
    /// # Errors
    /// Context B-rep construction, attribute allocation or identity exhaustion.
    pub fn new(k: &mut Kernel, target: &'target mut Brep) -> Result<Self> {
        let target_serial = crate::topology::brep::p3::notify_serial(&mut k.context, target)?;
        let mut pseudo = FilletBrep::new();
        // SmFilletBrep derives from SmBrep: use the actual context constructor,
        // including its context tolerances and construction notifications.
        *pseudo.brep_mut() = crate::topology::brep::p3::new_brep(
            k,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
        )?;
        let mut attributes = AttributeStore::new();
        let attribute = attributes.insert(Attribute::new(
            ids::ALL_FILLET_EDGES,
            Behavior::StandaloneReference,
        ))?;
        let executive = FilletExecutive::new(
            Some(FilletObject::Body(FilletBody::Target)),
            ExecutiveObjects {
                executive: FilletObject::Executive,
                pseudo_brep: FilletObject::Body(FilletBody::Pseudo),
                edge_attribute: FilletObject::Attribute(attribute),
            },
        );
        Ok(Self {
            executive,
            objects: SessionObjects {
                target,
                pseudo,
                intermediate: None,
                insertion_curves: insertion::curves::InsertionCurves::default(),
                notification_records: BTreeMap::new(),
                solvers: Vec::new(),
                corners: Vec::new(),
                generators: Vec::new(),
                laws: Vec::new(),
                attributes,
                merge_attributes: None,
                extended_surfaces: BTreeMap::new(),
                edge_caches: BTreeMap::new(),
                target_serial,
                self_intersection_handler: None,
            },
        })
    }

    /// Borrow the actual extended surface registered for a target face.
    /// Faces and this owning map belong to this session's target identity domain.
    #[must_use]
    pub fn extended_surface(&self, face: FaceId) -> Option<&Surface> {
        self.objects.extended_surfaces.get(&face)
    }

    /// Retain an actual target-face extension using the native domain-selection rule.
    /// The session owns the selected surface; later producers and readers share this map.
    ///
    /// # Errors
    /// A face absent from the actual target.
    pub fn add_extended_original_surface(&mut self, face: FaceId, surface: Surface) -> Result<()> {
        self.objects.target.face(face)?;
        FilletCorner::add_extended_original_surface_with_map(
            &mut self.objects.extended_surfaces,
            face,
            surface,
        );
        Ok(())
    }

    /// The actual executive, for shared reads.
    #[must_use]
    pub const fn executive(&self) -> &FilletExecutive<FilletObject> {
        &self.executive
    }

    /// The real target (kept exclusively borrowed until the session is dropped).
    #[must_use]
    pub fn target(&self) -> &Brep {
        self.objects.target
    }

    /// The target entity's actual identity issued by the kernel context.
    /// This is the same canonical identity used by context-owned notifications.
    #[must_use]
    pub fn target_identity(&self, entity: EntityRef) -> crate::geom::object::ObjectRef {
        crate::topology::brep::p3::topology_ref(self.objects.target_serial, entity)
    }

    /// The actual pseudo-B-rep that owns the solver geometries.
    #[must_use]
    pub const fn pseudo_brep(&self) -> &FilletBrep {
        &self.objects.pseudo
    }

    /// The actual attribute store, including the executive's fillet-edge attribute.
    #[must_use]
    pub const fn attributes(&self) -> &AttributeStore {
        &self.objects.attributes
    }

    /// Read a retained concrete solver, without copying its base.
    ///
    /// # Errors
    /// An absent or failed-construction solver slot.
    pub fn solver(&self, id: FilletSolverId) -> Result<&SolverObject> {
        self.objects.solver(id)
    }

    /// Read an actual retained corner.
    ///
    /// # Errors
    /// A corner not issued by this session.
    pub fn corner(&self, id: FilletCornerId) -> Result<&FilletCorner> {
        self.objects
            .corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live in this session"))
    }

    /// Retain a real cross-section generator, as the executive takes ownership.
    ///
    /// # Errors
    /// Generator identity exhaustion.
    pub fn add_surface_generator(
        &mut self,
        generator: FilletSurfaceGenerator,
    ) -> Result<FilletObject> {
        let object = self.objects.allocate_generator(generator)?;
        self.executive.add_fillet_surface_generator(object);
        Ok(object)
    }

    /// Construct `SmConstantRadiusFS` on the actual target sector and pseudo-B-rep.
    ///
    /// # Errors
    /// Solver identity exhaustion or the real native constructor's query failures.
    #[allow(clippy::too_many_arguments)] // the native constructor's parameters
    pub fn new_constant_radius_solver(
        &mut self,
        k: &Kernel,
        edgeuse: EdgeuseId,
        radius: f64,
        approx_tol: f64,
        ang_tol_rad: f64,
        tan_tol_rad: f64,
    ) -> Result<FilletSolverId> {
        self.objects.allocate_constant_radius_solver(
            k,
            edgeuse,
            radius,
            approx_tol,
            ang_tol_rad,
            tan_tol_rad,
        )
    }

    /// Assign a retained generator's descriptor to the actual solver base.
    /// This copies the port's immutable value descriptor; it does not promise native
    /// shared mutable generator-pointer identity.
    ///
    /// # Errors
    /// Unresolved solver or generator identities.
    pub fn set_solver_generator(
        &mut self,
        solver: FilletSolverId,
        generator: FilletObject,
    ) -> Result<()> {
        self.objects.assign_generator(solver, generator)
    }

    /// Run the exact constant-radius `SmApiFilletEdges` setup through
    /// `SetFilletParameters`, before topology/geometry execution.
    ///
    /// Zero selects linear, two blend-curve, every other cross-section code circular.
    /// All four tolerance arguments use the native `-1.0` defaults. The native flag
    /// reset in `SetFilletParameters` is retained after the wrapper's global-merge write.
    ///
    /// # Errors
    /// Native invalid input for an empty edge list; real parameter/constructor/loader errors.
    pub fn prepare_constant_radius_edges(
        &mut self,
        k: &mut Kernel,
        edges: &[EdgeId],
        radius: f64,
        xsect_type: u32,
        continuity: u32,
        thumbweight: f64,
    ) -> Result<()> {
        if edges.is_empty() {
            return Err(SolidError::Upstream {
                operation: OP,
                code: crate::error::SM_ERR_INVALID_INPUT,
                entity: None,
            });
        }
        let xsect = match xsect_type {
            0 => FilletSurfaceGeneratorType::Linear,
            2 => FilletSurfaceGeneratorType::BlendCurve,
            _ => FilletSurfaceGeneratorType::Circular,
        };
        let edges = edges
            .iter()
            .map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(*id)))
            .collect::<Vec<_>>();
        self.executive.set_do_global_merge(true);
        self.executive.set_fillet_parameters(
            k,
            &mut self.objects,
            &FilletParameters {
                brep: Some(FilletObject::Body(FilletBody::Target)),
                edges: &edges,
                xsect_types: &[xsect],
                radius_types: &[FilletSolverType::ConstRadius],
                radius_values: &[radius],
                var_rad_laws: None,
                approx_tol: -1.0,
                ang_tol_deg: -1.0,
                tan_tol_deg: -1.0,
                xsect_tol: -1.0,
                continuity,
                thumbweight,
            },
        )
    }

    /// `SetDoGlobalMerge(TRUE)` then the per-edge `SetFilletParameters` overload
    /// (`SmFilletExecutive.cpp:865`) with constant-radius solvers, as
    /// `SmApiFilletEdgesPerEdge` does: one cross-section type and radius per edge
    /// (or one for all), default tolerances and no laws.
    ///
    /// # Errors
    /// Native invalid input for an empty edge list or mismatched list sizes; real
    /// parameter/constructor/loader errors.
    pub fn prepare_per_edge_constant_radius(
        &mut self,
        k: &mut Kernel,
        edges: &[EdgeId],
        radii: &[f64],
        xsect_types: &[FilletSurfaceGeneratorType],
        continuity: u32,
        thumbweight: f64,
    ) -> Result<()> {
        if edges.is_empty() {
            return Err(SolidError::Upstream {
                operation: OP,
                code: crate::error::SM_ERR_INVALID_INPUT,
                entity: None,
            });
        }
        let radius_types = vec![FilletSolverType::ConstRadius; edges.len()];
        let edges = edges
            .iter()
            .map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(*id)))
            .collect::<Vec<_>>();
        self.executive.set_do_global_merge(true);
        self.executive.set_fillet_parameters(
            k,
            &mut self.objects,
            &FilletParameters {
                brep: Some(FilletObject::Body(FilletBody::Target)),
                edges: &edges,
                xsect_types,
                radius_types: &radius_types,
                radius_values: radii,
                var_rad_laws: None,
                approx_tol: -1.0,
                ang_tol_deg: -1.0,
                tan_tol_deg: -1.0,
                xsect_tol: -1.0,
                continuity,
                thumbweight,
            },
        )
    }

    /// `SetDoGlobalMerge(TRUE)` then the same-parameters `SetFilletParameters`
    /// overload with `SM_FS_VARIABLE_RADIUS`, as `SmApiVariableRadiusFillet` does:
    /// one cross-section type for every edge, `radius` as the (unused) solver
    /// distance, `law` shared by every edge's solver and default tolerances.
    ///
    /// Zero selects linear, two blend-curve, every other cross-section code circular.
    ///
    /// # Errors
    /// Native invalid input for an empty edge list; real parameter/constructor/loader errors.
    pub fn prepare_variable_radius_edges(
        &mut self,
        k: &mut Kernel,
        edges: &[EdgeId],
        radius: f64,
        law: FilletLaw,
        xsect_type: u32,
        continuity: u32,
    ) -> Result<()> {
        if edges.is_empty() {
            return Err(SolidError::Upstream {
                operation: OP,
                code: crate::error::SM_ERR_INVALID_INPUT,
                entity: None,
            });
        }
        let xsect = match xsect_type {
            0 => FilletSurfaceGeneratorType::Linear,
            2 => FilletSurfaceGeneratorType::BlendCurve,
            _ => FilletSurfaceGeneratorType::Circular,
        };
        let law = self.objects.allocate_law(law)?;
        // The same-parameters overload passes the one law to every edge's solver.
        let laws = vec![Some(law); edges.len()];
        let edges = edges
            .iter()
            .map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(*id)))
            .collect::<Vec<_>>();
        self.executive.set_do_global_merge(true);
        self.executive.set_fillet_parameters(
            k,
            &mut self.objects,
            &FilletParameters {
                brep: Some(FilletObject::Body(FilletBody::Target)),
                edges: &edges,
                xsect_types: &[xsect],
                radius_types: &[FilletSolverType::VariableRadius],
                radius_values: &[radius],
                var_rad_laws: Some(&laws),
                approx_tol: -1.0,
                ang_tol_deg: -1.0,
                tan_tol_deg: -1.0,
                xsect_tol: -1.0,
                continuity,
                thumbweight: 1.0,
            },
        )
    }

    /// Create and retain the actual corners in native executive order.
    ///
    /// # Errors
    /// A missing pending entry, undefined native null-vertex access, a failed factory
    /// query or exhausted corner identities. Earlier successful corners stay stored.
    pub fn create_corners(
        &mut self,
        k: &Kernel,
        error_info: Option<&mut ErrorInfo<FilletObject>>,
    ) -> Result<()> {
        let SessionObjects {
            target,
            solvers,
            corners,
            ..
        } = &mut self.objects;
        self.executive.create_fillet_corners(
            k,
            |k, executive, vertex, edgeuses| {
                let Some(FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(vertex))) =
                    vertex
                else {
                    return Err(invalid(
                        "native corner factory requires an actual target vertex",
                    ));
                };
                let edgeuses = edgeuses
                    .iter()
                    .map(|object| match object {
                        FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(id)) => {
                            Ok(*id)
                        }
                        _ => Err(invalid("corner edge use is not in the target")),
                    })
                    .collect::<Result<Vec<_>>>()?;
                let made = FilletCorner::create_with_solver_lookup(
                    k,
                    target,
                    vertex,
                    &edgeuses,
                    |edgeuse| {
                        let object =
                            FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(edgeuse));
                        let Some(FilletObject::Solver(id)) =
                            executive.fillet_solver_of_edgeuse(object)
                        else {
                            return Err(invalid("corner edge use has no loaded solver"));
                        };
                        solvers
                            .get(id.0 as usize)
                            .and_then(Option::as_ref)
                            .map(SolverObject::base)
                            .ok_or_else(|| invalid("corner solver is not live"))
                    },
                )?;
                let Some(corner) = made else {
                    return Ok(None);
                };
                let id = FilletCornerId(
                    u32::try_from(corners.len()).map_err(|_| invalid("too many corners"))?,
                );
                corners.push(corner);
                Ok(Some(FilletObject::Corner(id)))
            },
            error_info,
        )
    }

    /// Build one actual `NByOne` corner's pseudo-topology.
    ///
    /// This is a single-corner operation: the executive interleaves topology and
    /// vertex geometry per corner, so this does not batch-reorder those stages.
    ///
    /// # Errors
    /// Unresolved corner/solver identities or the real concrete topology producer's errors.
    pub fn make_nx1_corner_topology(&mut self, k: &Kernel, id: FilletCornerId) -> Result<()> {
        self.objects
            .make_nx1_corner_topology(k, id, &mut self.executive)
    }

    /// Calculate one ordinary `NByOne` corner's vertices using its retained solver.
    ///
    /// This follows the shared corner loop, actual derived solver operations and
    /// geometry-owned offset surfaces. Variable-law adjustment remains unwired.
    ///
    /// # Errors
    /// Actual topology/solver resolution errors, native calculation failures, or a
    /// genuine unsupported live-context dependency reached by the calculation.
    pub fn calc_nx1_corner_vert_geom(&mut self, k: &Kernel, id: FilletCornerId) -> Result<()> {
        self.objects.calc_nx1_corner_vert_geom(k, id)
    }

    /// Run the actual executive corner-vertex component in native corner order.
    ///
    /// Each corner's topology, calculation flag and vertex geometry complete before
    /// the next corner starts. Ordinary `NByOne`, `NByTwo` and convex `NxN` corners are
    /// supported (a mixed-convexity 4x2, tangent 4x2 or blending Nx2 adjustment stays
    /// unported); other concrete corner context remains unported. Smoothing and later adjustment are separate.
    ///
    /// # Errors
    /// Actual corner operations or lookups, with native failure metadata retained.
    pub fn create_corner_vertices(&mut self, k: &mut Kernel) -> Result<()> {
        self.executive.create_corner_vertices(k, &mut self.objects)
    }

    /// Run post-vertex corner topology adjustment on the actual retained objects.
    ///
    /// Ordinary nondegenerate corners use the inherited native base operation.
    /// Degenerate mutation and other concrete adjustment contexts remain unported.
    ///
    /// # Errors
    /// Actual point/state lookup failures or missing concrete mutation context.
    pub fn adjust_corner_topology(&mut self, k: &mut Kernel) -> Result<()> {
        self.executive.adjust_corner_topology(k, &mut self.objects)
    }

    /// Calculate one retained solver's fillet geometry using its actual context.
    ///
    /// Call after the executive's complete corner-vertex and adjustment stages.
    /// Constant-radius operations use the same owned solver, endpoint corners and
    /// pseudo-B-rep; unsupported concrete operation contexts remain explicit.
    ///
    /// # Errors
    /// Actual solver/corner resolution, native geometry failures or missing callees.
    pub fn calc_solver_geometry(&mut self, k: &Kernel, id: FilletSolverId) -> Result<()> {
        self.objects.calc_solver_geometry(k, id, &self.executive)
    }

    /// Calculate all actual solver geometries through the shared executive stage.
    ///
    /// # Errors
    /// Actual solver failures with native failure metadata, or missing dependencies.
    pub fn calculate_solver_geometry(&mut self, k: &mut Kernel) -> Result<()> {
        self.executive
            .calculate_solver_geometry(k, &mut self.objects)
    }

    /// Complete every corner's post-surface geometry in native corner order.
    /// Tangent adjustment and repeated vertex calculation precede each edge loop.
    ///
    /// # Errors
    /// Actual geometry failures or unsupported concrete operation contexts.
    pub fn calculate_corner_edges(&mut self, k: &mut Kernel) -> Result<()> {
        self.executive.calculate_corner_edges(k, &mut self.objects)
    }

    /// Trim the actual solvers' rail curves after all corner geometry is complete.
    ///
    /// # Errors
    /// Actual solver/geometry lookup or native trimming failures.
    pub fn trim_solver_rails(&mut self, k: &mut Kernel) -> Result<()> {
        self.executive.trim_solver_rails(k, &mut self.objects)
    }

    /// Run the executive's smooth-sequence prelude on its retained solvers.
    ///
    /// # Errors
    /// Pending corner entries (when `create_corners` has not run yet) or smooth-sequence
    /// curve construction errors.
    pub fn process_smooth_edge_sequences(&mut self, k: &mut Kernel) -> Result<()> {
        self.objects.creation_smooth(k, &mut self.executive)
    }

    /// Classify real solver rails and repair any adjusted corners in native order.
    ///
    /// # Errors
    /// Real classification/recovery failures, or a genuine missing recovery context.
    pub fn classify_solver_rails(&mut self, k: &mut Kernel) -> Result<()> {
        self.executive.classify_solver_rails(k, &mut self.objects)
    }

    /// Classify rails through the same shared stage and return actual adjusted-corner IDs.
    /// The referenced corners remain retained in this session after their repair.
    ///
    /// # Errors
    /// The same operation failures as [`Self::classify_solver_rails`].
    pub fn classify_solver_rails_with_adjusted(
        &mut self,
        k: &mut Kernel,
    ) -> Result<Vec<FilletObject>> {
        self.executive
            .classify_solver_rails_with_adjusted(k, &mut self.objects)
    }

    /// Allocate the actual intermediate B-rep and construct its corner and solver faces.
    ///
    /// # Errors
    /// Native face-construction failures, or a genuine missing corner-patch context.
    pub fn build_fillet_faces(&mut self, k: &mut Kernel) -> Result<()> {
        self.executive
            .build_fillet_faces(k, &mut self.objects)
            .map(|_| ())
    }

    /// The actual intermediate face body, absent until its native allocation stage.
    #[must_use]
    pub fn intermediate_brep(&self) -> Option<&Brep> {
        self.objects.intermediate.as_ref()
    }

    /// Borrow the actual fillet surface from its current owned or face-managed location.
    ///
    /// # Errors
    /// Missing geometry, an unavailable owning body, a foreign issued domain, or a
    /// partial face whose installed surface no longer exists.
    pub fn fillet_surface(
        &self,
        geom: crate::blend::fillet_geom::p1::FilletGeomId,
    ) -> Result<Option<&Surface>> {
        self.objects.fillet_surface(geom)
    }

    /// Resolve topology in the actual intermediate body's issued identity domain.
    ///
    /// # Errors
    /// The intermediate body has not been allocated or has no issued identity.
    pub fn intermediate_identity(
        &self,
        entity: EntityRef,
    ) -> Result<crate::geom::object::ObjectRef> {
        self.objects.intermediate_identity(entity)
    }

    /// Attach the real executive edge attribute and stitch the actual intermediate body.
    /// Successful counts and gaps describe this operation; failures retain prior mutations.
    ///
    /// # Errors
    /// Native stitching failure or a genuine missing mutation-context capability.
    pub fn stitch_fillet_faces(&mut self, k: &mut Kernel) -> Result<StitchAttempt> {
        self.executive
            .stitch_fillet_faces(k, &mut self.objects, FilletObject::IntermediateBrep)
    }

    /// Shrink the actual surface-owner face of each solver's first geometry.
    /// This component does not change the caller's Boolean/fillet context flags.
    ///
    /// # Errors
    /// Missing actual ownership or shrink context; ordinary native shrink errors
    /// are ignored by the shared executive algorithm.
    pub fn shrink_fillet_faces(&mut self, k: &mut Kernel) -> Result<()> {
        self.executive.shrink_fillet_faces(k, &mut self.objects)
    }

    /// Run the complete native intermediate fillet-body construction algorithm
    /// after solver setup and [`Self::create_corners`].
    /// This constructor ends after attribute attachment and stitching; it does not
    /// merge the intermediate body into the target or complete the public fillet API.
    ///
    /// # Errors
    /// The original stage error or genuine missing live capability, retaining its
    /// actual mutation prefix. Optional error information is assigned only on success.
    pub fn create_fillet_brep(
        &mut self,
        k: &mut Kernel,
        error_info: Option<&mut ErrorInfo<FilletObject>>,
    ) -> Result<()> {
        self.executive
            .create_fillet_brep(k, &mut self.objects, error_info)
    }

    /// Map actual pseudo fillet edges to the constructed intermediate body's edges.
    /// The native mapping algorithm reads candidate raw intervals without repairing them.
    ///
    /// # Errors
    /// Native curve-evaluation/mapping errors, preserving prior mappings and error notes.
    pub fn set_fillet_brep_edge_map(&mut self, k: &mut Kernel) -> Result<()> {
        self.executive
            .set_fillet_brep_edge_map(k, &mut self.objects)
    }

    /// Load this actual solver through the native executive algorithm.
    ///
    /// # Errors
    /// The real loader's surface, cache, topology or lifecycle errors; prior writes remain.
    pub fn load_solver(&mut self, k: &Kernel, solver: FilletSolverId) -> Result<()> {
        let result =
            self.executive
                .load_fillet_solver(k, &mut self.objects, FilletObject::Solver(solver));
        self.sync_self_intersection_handler();
        result
    }

    /// `SetSelfIntersectionHandler`: retain `handler` as the executive's handler,
    /// which every loaded solver reads through its executive.
    pub fn set_self_intersection_handler(&mut self, handler: Option<MakeSurfaceBlendSih>) {
        self.objects.self_intersection_handler = handler;
        self.executive
            .set_self_intersection_handler(handler.map(|_| FilletObject::SelfIntersectionHandler));
        self.sync_self_intersection_handler();
    }

    /// The retained self-intersection handler (`m_pSelfIntersectionHandler`).
    #[must_use]
    pub const fn self_intersection_handler(&self) -> Option<MakeSurfaceBlendSih> {
        self.objects.self_intersection_handler
    }

    /// Make every solver with an executive see the retained handler, as upstream
    /// solvers read `m_pExecutive->m_pSelfIntersectionHandler` on each use.
    pub(super) fn sync_self_intersection_handler(&mut self) {
        let algorithm = self
            .objects
            .self_intersection_handler
            .as_ref()
            .map(MakeSurfaceBlendSih::algorithm_type);
        for solver in self.objects.solvers.iter_mut().flatten() {
            let base = solver.base_mut();
            if base.executive.is_some() {
                base.executive_self_intersection_handler = algorithm;
            }
        }
    }

    /// `SetDoGlobalMerge`.
    pub fn set_do_global_merge(&mut self, do_global_merge: bool) {
        self.executive.set_do_global_merge(do_global_merge);
    }

    /// `SmEdge::GetBlendEdgeuse` of a target edge.
    ///
    /// # Errors
    /// Those of [`crate::topology::edge::blend_edgeuse`].
    pub fn blend_edgeuse(&mut self, k: &Kernel, edge: EdgeId) -> Result<Option<EdgeuseId>> {
        crate::topology::edge::blend_edgeuse(k, self.objects.target, edge)
    }

    /// Construct `SmConstantDistanceFS` on the actual target sector and pseudo-B-rep.
    ///
    /// # Errors
    /// Solver identity exhaustion or the real native constructor's query failures.
    pub fn new_constant_distance_solver(
        &mut self,
        k: &Kernel,
        edgeuse: EdgeuseId,
        distance: f64,
        approx_tol: f64,
        ang_tol_rad: f64,
        tan_tol_rad: f64,
    ) -> Result<FilletSolverId> {
        self.objects.allocate_constant_distance_solver(
            k,
            edgeuse,
            distance,
            approx_tol,
            ang_tol_rad,
            tan_tol_rad,
        )
    }

    /// `GetFilletCorners`: the executive's corners in creation order.
    ///
    /// # Errors
    /// A corner list entry that is not a session corner.
    pub fn fillet_corners(&self) -> Result<Vec<FilletCornerId>> {
        self.executive
            .corners
            .iter()
            .map(|object| match object {
                FilletObject::Corner(id) => Ok(*id),
                _ => Err(invalid("executive corner is not a session corner")),
            })
            .collect()
    }

    /// `SmFilletCorner::SetBevel` on one live corner.
    ///
    /// # Errors
    /// An unresolved corner identity.
    pub fn set_corner_bevel(&mut self, id: FilletCornerId, is_bevel: bool) -> Result<()> {
        self.objects
            .corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live in this session"))?
            .set_bevel(is_bevel);
        Ok(())
    }

    /// `SetBevelCorner(pVertex)`: make the corner of a target vertex a bevel.
    ///
    /// # Errors
    /// Those of [`FilletExecutive::set_bevel_corner`]: `SM_ERR_NULL_POINTER` for a
    /// vertex without an entry, invalid input for one without a created corner.
    pub fn set_bevel_corner(&mut self, vertex: VertexId) -> Result<()> {
        let corners = &mut self.objects.corners;
        self.executive.set_bevel_corner(
            Some(FilletObject::Topology(
                FilletBody::Target,
                EntityRef::Vertex(vertex),
            )),
            |object| {
                let FilletObject::Corner(id) = object else {
                    return Err(invalid("object is not a corner"));
                };
                corners
                    .get_mut(id.0 as usize)
                    .ok_or_else(|| invalid("corner is not live in this session"))?
                    .set_bevel(true);
                Ok(())
            },
        )
    }
}

impl CornerEdgeAccess<FilletObject> for SessionObjects<'_> {
    fn edges_adjust_tangent(
        &mut self,
        k: &mut Kernel,
        corner: FilletObject,
        _executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        let SessionObjects {
            target, corners, ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        corner.adjust_tangent_corner_with(
            &mut (),
            |corner, ()| {
                if !corner.is_nx2_tangent_adjustment_corner(k, target)? {
                    return Ok(());
                }
                // UNPORTED(blend.fillet_executive_p1): actual Nx2 tangent-adjustment context.
                Err(SolidError::unported(
                    "SmFilletNx2Corner::AdjustTangentCorner (live context)",
                    "blend.fillet_executive_p1",
                ))
            },
            |_, ()| {
                // UNPORTED(blend.fillet_executive_p1): actual closed Nx1 tangent-adjustment context.
                Err(SolidError::unported(
                    "SmFilletNx1ClosedCorner::AdjustTangentCorner (live context)",
                    "blend.fillet_executive_p1",
                ))
            },
        )
    }
    fn edges_calculate(
        &mut self,
        k: &mut Kernel,
        corner: FilletObject,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            extended_surfaces,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let mut context = SessionCornerEdgeContext {
            k,
            target,
            pseudo,
            lookup: SessionEdgeLookup {
                solvers,
                extended_surfaces,
            },
            corner: id,
            do_global_merge: &executive.options.global_merge,
        };
        corner.calc_corner_edge_geom_with(
            &mut context,
            |context, corner, edge| {
                let corners = CornerLookup {
                    table: None,
                    this: Some((Some(context.corner), corner)),
                    exec: None,
                };
                context.pseudo.calc_corner_edge_geom_with_context(
                    context.k,
                    edge,
                    corners,
                    &context.lookup,
                    context.target,
                )
            },
            |context| *context.do_global_merge,
            |context| &mut *context.pseudo,
        )
    }
}

impl EdgeMapAccess<FilletObject> for SessionObjects<'_> {
    fn mapping_edges(&self, body: FilletObject) -> Result<Vec<FilletObject>> {
        FaceStitchAccess::stitch_edges(self, body)
    }
    fn mapping_solver_geoms(&self, solver: FilletObject) -> Result<Vec<FilletObject>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("mapping solver is not retained"));
        };
        Ok(self
            .solver(id)?
            .base()
            .fillet_geoms
            .iter()
            .copied()
            .map(FilletObject::Geometry)
            .collect())
    }
    fn mapping_geom_rails(&self, geom: FilletObject) -> Result<Vec<Option<FilletObject>>> {
        let FilletObject::Geometry(id) = geom else {
            return Err(invalid("mapping geometry is not retained"));
        };
        let geom = self.pseudo.geom(id)?;
        Ok(geom
            .rails
            .iter()
            .copied()
            .chain(geom.other_rails1.iter().copied().map(Some))
            .chain(geom.other_rails2.iter().copied().map(Some))
            .map(|edge| {
                edge.map(|edge| FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edge(edge)))
            })
            .collect())
    }
    fn mapping_side_edgeuses(&self, geom: FilletObject) -> Result<Vec<FilletObject>> {
        let FilletObject::Geometry(id) = geom else {
            return Err(invalid("mapping geometry is not retained"));
        };
        Ok(self
            .pseudo
            .geom(id)?
            .side_eus
            .iter()
            .copied()
            .map(|eu| FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edgeuse(eu)))
            .collect())
    }
    fn mapping_edgeuse_edge(&self, edgeuse: FilletObject) -> Result<Option<FilletObject>> {
        let FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edgeuse(eu)) = edgeuse else {
            return Err(invalid("mapping edgeuse is not actual pseudo topology"));
        };
        Ok(self
            .pseudo
            .brep()
            .edgeuse(eu)?
            .edge
            .map(|edge| FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edge(edge))))
    }
    fn mapping_original_face(&self, edge: FilletObject) -> Result<Option<FilletObject>> {
        Ok(self
            .mapping_edge(edge)?
            .original_face()
            .map(|face| FilletObject::Topology(FilletBody::Target, EntityRef::Face(face))))
    }
    fn mapping_original_edge(&self, edge: FilletObject) -> Result<Option<FilletObject>> {
        Ok(self
            .mapping_edge(edge)?
            .original_edge()
            .map(|edge| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge))))
    }
    fn mapping_corner_edges(&self, corner: FilletObject) -> Result<Vec<Option<FilletObject>>> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("mapping corner is not retained"));
        };
        let corner = self
            .corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("mapping corner is not live"))?;
        Ok(corner
            .edges
            .iter()
            .copied()
            .map(|edge| {
                Some(FilletObject::Topology(
                    FilletBody::Pseudo,
                    EntityRef::Edge(edge),
                ))
            })
            .collect())
    }
    fn mapping_setback(&self, corner: FilletObject) -> Result<f64> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("mapping corner is not retained"));
        };
        Ok(self
            .corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("mapping corner is not live"))?
            .set_back_dist())
    }
    fn mapping_note(
        &self,
        edge: FilletObject,
    ) -> Result<(
        Option<&'static str>,
        crate::blend::fillet_geom::p1::FilletEdgeType,
        crate::blend::fillet_geom::p1::FilStatus,
    )> {
        let edge = self.mapping_edge(edge)?;
        Ok((edge.name(), edge.fillet_edge_type(), edge.status()))
    }
    fn mapping_find(
        &mut self,
        k: &mut Kernel,
        edge: FilletObject,
        candidates: &[FilletObject],
        multiple: bool,
    ) -> Result<()> {
        let FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edge(edge)) = edge else {
            return Err(invalid("mapping edge is not actual pseudo topology"));
        };
        let candidates = candidates
            .iter()
            .map(|object| {
                let FilletObject::IntermediateTopology(EntityRef::Edge(edge)) = *object else {
                    return Err(invalid(
                        "mapping candidate is not actual intermediate topology",
                    ));
                };
                Ok(edge)
            })
            .collect::<Result<Vec<_>>>()?;
        let body = self
            .intermediate
            .as_ref()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        self.pseudo
            .find_fillet_brep_edge(k, edge, body, &candidates, multiple)
    }
}

impl crate::blend::fillet_executive::p1::SmoothSequenceAccess<FilletObject> for SessionObjects<'_> {
    fn smooth_edge_edgeuses(&self, edge: FilletObject) -> Result<Vec<FilletObject>> {
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)) = edge else {
            return Err(invalid("smooth-sequence edge is not in the target"));
        };
        Ok(
            crate::topology::edge::edgeuses(self.target, id, None, None)?
                .into_iter()
                .map(|eu| FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(eu)))
                .collect(),
        )
    }

    fn smooth_solver_edge(&self, solver: FilletObject) -> Result<FilletObject> {
        self.vertices_solver_edge(solver)
    }

    fn smooth_solver_corner(
        &self,
        executive: &FilletExecutive<FilletObject>,
        solver: FilletObject,
        index: usize,
    ) -> Result<Option<FilletObject>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let s = self
            .solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("solver is not live"))?;
        let Some(vertex) = s.base().vertex(self.target, index)? else {
            return Ok(None);
        };
        executive.fillet_corner_of_vertex(FilletObject::Topology(
            FilletBody::Target,
            EntityRef::Vertex(vertex),
        ))
    }

    fn smooth_corner_tangent_solver(
        &self,
        k: &Kernel,
        executive: &FilletExecutive<FilletObject>,
        corner: FilletObject,
        solver: FilletObject,
    ) -> Result<Option<FilletObject>> {
        let FilletObject::Solver(sid) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let corner = self.corner_object(corner)?;
        let tangent =
            corner.tangent_fillet_solver_with_lookup(k, self.target, sid, |eu| match executive
                .fillet_solver_of_edgeuse(FilletObject::Topology(
                    FilletBody::Target,
                    EntityRef::Edgeuse(eu),
                )) {
                Some(FilletObject::Solver(id)) => Some(id),
                _ => None,
            })?;
        Ok(tangent.map(FilletObject::Solver))
    }

    fn smooth_solver_edge0_id(&self, solver: FilletObject) -> Result<EdgeId> {
        let edge = self.vertices_solver_edge(solver)?;
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)) = edge else {
            return Err(invalid("solver edge is not in the target"));
        };
        Ok(id)
    }

    fn smooth_target_brep(&self) -> Result<&Brep> {
        Ok(&*self.target)
    }

    fn smooth_with_target_mut<T>(
        &mut self,
        edges: &[EdgeId],
        f: impl FnOnce(&mut Brep) -> Result<T>,
    ) -> Result<T> {
        for &edge in edges {
            self.edge_caches.remove(&edge);
        }
        f(self.target)
    }

    fn smooth_set_edge_curve(
        &mut self,
        k: &mut Kernel,
        edge: EdgeId,
        curve: crate::curve::curve::Curve,
    ) -> Result<()> {
        self.edge_caches.remove(&edge);
        let serial = self.target_serial;
        let key_of = |entity| crate::topology::brep::p3::topology_ref(serial, entity);
        crate::topology::brep::p3::seed_records(
            &mut self.notification_records,
            self.target,
            &key_of,
        );
        crate::topology::brep::p3::with_lent_context(k, |_k, context| {
            let mut env = crate::topology::edge::NotifyEnv {
                records: &mut self.notification_records,
                attributes: &mut self.attributes,
                context: Some(context),
                key_of: &key_of,
            };
            let _ = crate::topology::edge::set_curve(
                self.target,
                edge,
                Some(curve),
                crate::topology::edge::SetCurveOptions {
                    delete_old_curve: true,
                    dbg_warn_leaks: true,
                    call_notify: true,
                },
                &mut env,
                [None, None],
            )?;
            Ok(())
        })?
    }

    fn smooth_solver_original_curve(
        &self,
        solver: FilletObject,
    ) -> Result<Option<&crate::curve::curve::Curve>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let s = self
            .solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("solver is not live"))?;
        Ok(match s {
            SolverObject::VariableRadius(vr) => vr.curve_based.original_curve(),
            _ => None,
        })
    }

    fn smooth_set_solver_original_curve(
        &mut self,
        solver: FilletObject,
        curve: crate::curve::curve::Curve,
    ) -> Result<()> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let s = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live"))?;
        if let SolverObject::VariableRadius(vr) = s {
            vr.curve_based.set_original_curve(Some(curve));
        }
        Ok(())
    }

    fn smooth_set_solver_law_interval(
        &mut self,
        solver: FilletObject,
        interval: crate::geom::extent1d::Extent1d,
    ) -> Result<()> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let s = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live"))?;
        if let SolverObject::VariableRadius(vr) = s {
            vr.set_fillet_law_interval(interval);
        }
        Ok(())
    }
}

impl BrepCreationAccess<FilletObject> for SessionObjects<'_> {
    fn creation_smooth(
        &mut self,
        k: &mut Kernel,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        executive.process_smooth_edge_sequences_narrow(k, self)
    }
}

impl FaceShrinkAccess<FilletObject> for SessionObjects<'_> {
    fn shrink_geometry_face(&self, geom: FilletObject) -> Result<Option<FilletObject>> {
        use crate::blend::fillet_geom::p1::FilletSurfaceStorage;
        let FilletObject::Geometry(id) = geom else {
            return Err(invalid("shrink geometry is not actually retained"));
        };
        let Some(surface) = self.fillet_surface(id)? else {
            return Ok(None);
        };
        let Some(owner) = surface.owner() else {
            return Ok(None);
        };
        if owner.type_code != crate::topology::face::p2::TYPE {
            return Ok(None);
        }
        if let Some(FilletSurfaceStorage::ManagedFace { face, .. }) =
            self.pseudo.geom(id)?.fillet_surface.as_ref()
        {
            if owner == self.intermediate_identity(EntityRef::Face(*face))? {
                return Ok(Some(FilletObject::IntermediateTopology(EntityRef::Face(
                    *face,
                ))));
            }
        }
        // UNPORTED(blend.fillet_executive_p3): actual foreign face-owner resolver.
        Err(SolidError::unported(
            "SmFilletGeom::GetFilletSurface owner (live face context)",
            "blend.fillet_executive_p3",
        ))
    }
    fn shrink_actual_face(&mut self, k: &mut Kernel, face: FilletObject) -> Result<()> {
        let FilletObject::IntermediateTopology(EntityRef::Face(face)) = face else {
            return Err(invalid("shrink face is not actual intermediate topology"));
        };
        let serial = self
            .intermediate
            .as_ref()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?
            .notify_serial()
            .ok_or_else(|| invalid("intermediate body has no kernel identity"))?;
        let key_of = |entity| crate::topology::brep::p3::topology_ref(serial, entity);
        let body = self
            .intermediate
            .as_mut()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        crate::topology::brep::p3::seed_records(&mut self.notification_records, body, &key_of);
        crate::topology::brep::p3::with_lent_context(k, |k, context| {
            let mut env = crate::topology::edge::NotifyEnv {
                records: &mut self.notification_records,
                attributes: &mut self.attributes,
                context: Some(context),
                key_of: &key_of,
            };
            crate::topology::face::p1::shrink_geometry_with_notifications(k, body, face, &mut env)
        })?
    }
}

impl FaceStitchAccess<FilletObject> for SessionObjects<'_> {
    fn stitch_edges(&self, body: FilletObject) -> Result<Vec<FilletObject>> {
        if body != FilletObject::IntermediateBrep {
            return Err(invalid("stitch body is not the actual intermediate body"));
        }
        let body = self
            .intermediate
            .as_ref()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        Ok(crate::topology::brep::p1::edges(body, None)?
            .into_iter()
            .map(|e| FilletObject::IntermediateTopology(EntityRef::Edge(e)))
            .collect())
    }
    fn stitch_attribute(
        &mut self,
        edge: FilletObject,
        attribute: Option<FilletObject>,
    ) -> Result<()> {
        let FilletObject::IntermediateTopology(EntityRef::Edge(edge)) = edge else {
            return Err(invalid("stitch edge is not actual intermediate topology"));
        };
        let Some(attribute) = attribute else {
            return Ok(());
        };
        let FilletObject::Attribute(attribute) = attribute else {
            return Err(invalid(
                "stitch attribute is not retained in the actual store",
            ));
        };
        let body = self
            .intermediate
            .as_ref()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        body.edge(edge)?;
        let serial = body
            .notify_serial()
            .ok_or_else(|| invalid("intermediate body has no kernel identity"))?;
        self.attributes.add_attribute(
            crate::topology::brep::p3::topology_ref(serial, EntityRef::Edge(edge)).key,
            attribute,
        )
    }
    fn stitch_body(
        &mut self,
        k: &mut Kernel,
        body: FilletObject,
        tolerance: f64,
    ) -> Result<StitchAttempt> {
        if body != FilletObject::IntermediateBrep {
            return Err(invalid("stitch body is not the actual intermediate body"));
        }
        let body = self
            .intermediate
            .as_mut()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        let serial = body
            .notify_serial()
            .ok_or_else(|| invalid("intermediate body has no kernel identity"))?;
        let key_of = |entity| crate::topology::brep::p3::topology_ref(serial, entity);
        let keys = |object| match object {
            crate::topology::brep_cache::CacheObject::Brep => {
                key_of(EntityRef::Brep(crate::topology::BrepId::from_index(0)))
            }
            crate::topology::brep_cache::CacheObject::Other(owner) => owner,
            object => crate::topology::brep::p2::caller_cache_key(&key_of, object),
        };
        // This owned body binds curves and surfaces directly to its actual topology.
        // New intermediary geometry cannot enter through the unregistered split seam.
        let owner_of = |_| None;
        let identity = crate::query::brep::BrepIdentity {
            brep_id: crate::topology::BrepId::from_index(
                u32::try_from(serial)
                    .map_err(|_| invalid("intermediate body identity exceeds query domain"))?,
            ),
            keys: &keys,
            owner_of: &owner_of,
        };
        if k.context.issued_brep_serial(body.notify_identity_slot()) != Some(serial) {
            return Err(invalid(
                "stitch body belongs to another notification context",
            ));
        }
        crate::geom::context::Context::with_notification_context(k, |k, context| {
            crate::topology::brep::p3::seed_records(&mut self.notification_records, body, &key_of);
            let mut env = crate::topology::edge::NotifyEnv {
                records: &mut self.notification_records,
                attributes: &mut self.attributes,
                context: Some(context),
                key_of: &key_of,
            };
            let mut edits = crate::heal::stitch::p1::BorrowedStitchEdits {
                env: &mut env,
                curve_identity: crate::topology::brep::p9::CurveIdentity::Actual(
                    &crate::topology::brep::p9::issue_curve_identity,
                ),
            };
            StitchAttempt::from_stitch(crate::topology::brep::p4::stitch_faces_with_context(
                k,
                body,
                tolerance,
                None,
                false,
                Some(identity),
                Some(&mut edits),
            ))
        })?
    }
}

impl FaceConstructionAccess<FilletObject> for SessionObjects<'_> {
    fn faces_new_brep(&mut self, k: &mut Kernel) -> Result<FilletObject> {
        let mut body = crate::topology::brep::p3::new_brep(
            k,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
        )?;
        let serial = body
            .notify_serial()
            .ok_or_else(|| invalid("intermediate body has no kernel identity"))?;
        body.set_key(
            crate::topology::brep::p3::topology_ref(
                serial,
                EntityRef::Brep(crate::topology::BrepId::from_index(0)),
            )
            .key,
        );
        let key_of = |entity| crate::topology::brep::p3::topology_ref(serial, entity);
        crate::topology::brep::p3::seed_records(&mut self.notification_records, &body, &key_of);
        self.intermediate = Some(body);
        Ok(FilletObject::IntermediateBrep)
    }
    fn faces_tolerance(&self, _k: &Kernel, body: FilletObject) -> Result<f64> {
        match body {
            FilletObject::Body(FilletBody::Target) => Ok(self.target.tolerance()),
            FilletObject::IntermediateBrep => self
                .intermediate
                .as_ref()
                .map(Brep::tolerance)
                .ok_or_else(|| invalid("intermediate body is not allocated")),
            _ => Err(invalid("face tolerance object is not an actual body")),
        }
    }
    fn faces_set_tolerance(
        &mut self,
        k: &mut Kernel,
        body: FilletObject,
        tolerance: f64,
    ) -> Result<()> {
        if body != FilletObject::IntermediateBrep {
            return Err(invalid(
                "face tolerance destination is not the intermediate body",
            ));
        }
        let body = self
            .intermediate
            .as_mut()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        crate::topology::brep::p2::set_tolerance(k, body, tolerance, true, true)
    }
    fn faces_enable_editing(&mut self, body: FilletObject) -> Result<()> {
        if body != FilletObject::IntermediateBrep {
            return Err(invalid(
                "face editing destination is not the intermediate body",
            ));
        }
        self.intermediate
            .as_mut()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?
            .set_editing_enabled(true);
        Ok(())
    }
    fn faces_corner_geometry(
        &mut self,
        k: &mut Kernel,
        corner: FilletObject,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("corner patch object is not an actual corner"));
        };
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            intermediate,
            notification_records,
            attributes,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner patch is not live"))?;
        corner.calc_corner_geom_with(&mut (), |corner, ()| {
            match corner.kind {
                FilletCornerKind::ThreeByTwoMixed
                | FilletCornerKind::NByOne
                | FilletCornerKind::Open
                | FilletCornerKind::NByTwo => {
                    // UNPORTED(blend.fillet_corner_p3): actual concrete corner-patch construction context.
                    return Err(SolidError::unported(
                        "SmFilletCorner::CreateCornerPatch (live construction context)",
                        "blend.fillet_corner_p3",
                    ));
                }
                _ => {}
            }
            // SmFilletCorner::CreateCornerPatch, building into m_pFilletBrep.
            let body = intermediate
                .as_mut()
                .ok_or_else(|| invalid("intermediate body is not allocated"))?;
            let serial = body
                .notify_serial()
                .ok_or_else(|| invalid("intermediate body has no kernel identity"))?;
            let key_of = |entity| crate::topology::brep::p3::topology_ref(serial, entity);
            crate::topology::brep::p3::seed_records(notification_records, body, &key_of);
            let store = SessionSolvers(solvers);
            crate::topology::brep::p3::with_lent_context(k, |k, context| {
                let mut fillet = CornerFilletBrep {
                    brep: body,
                    notify: crate::topology::edge::NotifyEnv {
                        records: notification_records,
                        attributes,
                        context: Some(context),
                        key_of: &key_of,
                    },
                };
                let mut note = |corner: &FilletCorner| {
                    executive.error.note_corner_error(
                        crate::blend::fillet_geom::p1::FilletErrorType::VertexProblem,
                        corner.corner_type(),
                        crate::blend::fillet_geom::p1::FilStatus::Failure,
                        Some(corner.kind.name()),
                        corner.filleted_vertex().map(|v| {
                            FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))
                        }),
                        [None, None], // For now, we don't generally have just two surfaces.
                        [None, None], // For now, we don't generally have just two edgeuses.
                        Some("Fillet Corner Error: Corner Face Creation Error"),
                    );
                };
                let mut env = CornerPatchEnv {
                    k,
                    target,
                    pseudo,
                    solvers: &store,
                    fillet: Some(&mut fillet),
                    note_face_error: &mut note,
                };
                corner.create_corner_patch_default_with(&mut env)
            })?
        })
    }
    fn faces_solver_tolerance(&self, solver: FilletObject) -> Result<f64> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("face solver is not an actual solver"));
        };
        Ok(self.solver(id)?.base().approx_tol_3d)
    }
    fn faces_solver_geom(
        &self,
        solver: FilletObject,
        index: usize,
    ) -> Result<Option<FilletObject>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("face solver is not an actual solver"));
        };
        Ok(self
            .solver(id)?
            .base()
            .fillet_geoms
            .get(index)
            .copied()
            .map(FilletObject::Geometry))
    }
    fn faces_make_face(
        &mut self,
        k: &mut Kernel,
        geom: FilletObject,
        body: FilletObject,
        _executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Geometry(id) = geom else {
            return Err(invalid("face geometry is not actually retained"));
        };
        if body != FilletObject::IntermediateBrep {
            return Err(invalid("face destination is not the intermediate body"));
        }
        let sid = self
            .pseudo
            .geom(id)?
            .fillet_solver()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let solver = self
            .solvers
            .get(sid.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("face geometry solver is not live"))?
            .base();
        let body = self
            .intermediate
            .as_mut()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        let serial = body
            .notify_serial()
            .ok_or_else(|| invalid("intermediate body has no kernel identity"))?;
        let key_of = |entity| crate::topology::brep::p3::topology_ref(serial, entity);
        crate::topology::brep::p3::seed_records(&mut self.notification_records, body, &key_of);
        crate::topology::brep::p3::with_lent_context(k, |k, context| {
            let mut env = crate::topology::edge::NotifyEnv {
                records: &mut self.notification_records,
                attributes: &mut self.attributes,
                context: Some(context),
                key_of: &key_of,
            };
            self.pseudo
                .make_face_brep_with_solver(k, id, sid, solver, body, &mut env)
        })?
    }
}

impl RailClassificationAccess<FilletObject> for SessionObjects<'_> {
    fn rails_classify(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        adjusted: &mut Vec<FilletObject>,
        _executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("rollover object is not an actual solver"));
        };
        let serial = self
            .pseudo
            .brep()
            .notify_serial()
            .ok_or_else(|| invalid("pseudo B-rep has no kernel identity"))?;
        let mut ownership = classification::Ownership {
            serial,
            brep: crate::topology::brep::p3::topology_ref(
                serial,
                EntityRef::Brep(crate::topology::BrepId::from_index(0)),
            ),
        };
        let solver = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("rollover solver is not live"))?;
        let mut context = classification::Context {
            corners: &self.corners,
        };
        let mut actual_adjusted = Vec::new();
        let result = solver.base_mut().test_roll_over_with_context(
            k,
            self.target,
            &mut self.pseudo,
            &mut context,
            &mut actual_adjusted,
            &mut ownership,
        );
        // Retain append-only adjusted-corner output even if a later rail fails.
        adjusted.extend(actual_adjusted.into_iter().map(FilletObject::Corner));
        result
    }
    fn rails_reinitialize_corner(
        &mut self,
        _k: &mut Kernel,
        corner: FilletObject,
        _executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("adjusted object is not an actual corner"));
        };
        let corner = self
            .corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("adjusted corner is not live"))?;
        corner.re_initialize_corner_edges(&mut self.pseudo)
    }
}

impl RailTrimAccess<FilletObject> for SessionObjects<'_> {
    fn rails_trim(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        _executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let SessionObjects {
            target,
            pseudo,
            solvers,
            ..
        } = self;
        let solver = solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("solver is not live in this session"))?
            .base();
        solver.trim_rails_with(|geometry, actual_solver| {
            pseudo.trim_rail_curves_with_solver(k, geometry, id, actual_solver, target)
        })
    }
}

impl SolverGeometryAccess<FilletObject> for SessionObjects<'_> {
    fn geometry_calculate(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        self.calc_solver_geometry(k, id, executive)
    }
    fn geometry_edge(&self, solver: FilletObject) -> Result<FilletObject> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let edgeuse = self
            .solver(id)?
            .base()
            .edgeuse(0)?
            .ok_or_else(|| invalid("solver without first edgeuse"))?;
        let edge = crate::topology::edgeuse::p1::edge(self.target, edgeuse)?
            .ok_or_else(|| invalid("solver edgeuse without edge"))?;
        Ok(FilletObject::Topology(
            FilletBody::Target,
            EntityRef::Edge(edge),
        ))
    }
}

impl SolverLoadingAccess<FilletObject> for SessionObjects<'_> {
    fn load_solver_base_mut(&mut self, solver: FilletObject) -> Result<&mut FilletSolver> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        Ok(self.solver_mut(id)?.base_mut())
    }
    fn load_solver_has_surface(&self, solver: FilletObject, rail: usize) -> Result<bool> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let base = self.solver(id)?.base();
        if let Some(geom) = base.fg_for_surfaces {
            Ok(self
                .pseudo
                .geom(geom)?
                .offset_surface(rail, Some(base))?
                .is_some())
        } else {
            Ok(base.surface(rail)?.is_some())
        }
    }
    fn load_edge_g1_knots(&mut self, k: &Kernel, edge: FilletObject) -> Result<Vec<f64>> {
        const LOADER: &str = "SmFilletExecutive::LoadFilletSolver";
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)) = edge else {
            return Err(invalid("solver edge is not in the target"));
        };
        let curve = self
            .target
            .edge(id)?
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(LOADER))?;
        let knots = curve.knots(k, None)?.0;
        if let std::collections::btree_map::Entry::Vacant(entry) = self.edge_caches.entry(id) {
            let cache = crate::curve::curve::p5::make_curve_cache(
                k,
                curve,
                Some(FilletObject::Curve(FilletBody::Target, id)),
            )
            .map_err(|error| {
                if error.is_unported() {
                    error
                } else {
                    SolidError::null_pointer(LOADER)
                }
            })?;
            entry.insert(cache);
        }
        self.edge_caches
            .get(&id)
            .ok_or_else(|| invalid("curve cache not retained"))?
            .continuities()
            .iter()
            .enumerate()
            .filter(|(_, continuity)| **continuity == ContinuityType::G1)
            .map(|(index, _)| {
                knots
                    .get(index)
                    .copied()
                    .ok_or_else(|| invalid("curve knot/cache count mismatch"))
            })
            .collect()
    }
    fn load_solver_edgeuse(
        &self,
        solver: FilletObject,
        rail: usize,
    ) -> Result<Option<FilletObject>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        Ok(self
            .solver(id)?
            .base()
            .edgeuse(rail)?
            .map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(id))))
    }
    fn load_edgeuse_edge(&self, edgeuse: FilletObject) -> Result<Option<FilletObject>> {
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(id)) = edgeuse else {
            return Err(invalid("edge use is not in the target"));
        };
        Ok(self
            .target
            .edgeuse(id)?
            .edge
            .map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id))))
    }
    fn load_edgeuse_vertex(&self, edgeuse: FilletObject) -> Result<Option<FilletObject>> {
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(id)) = edgeuse else {
            return Err(invalid("edge use is not in the target"));
        };
        let vertexuse = self
            .target
            .edgeuse(id)?
            .vertexuse
            .ok_or_else(|| invalid("edge use has no vertex use"))?;
        Ok(self
            .target
            .vertexuse(vertexuse)?
            .vertex
            .map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(id))))
    }
}

impl SessionObjects<'_> {
    fn allocate_generator(&mut self, generator: FilletSurfaceGenerator) -> Result<FilletObject> {
        let id =
            u32::try_from(self.generators.len()).map_err(|_| invalid("too many generators"))?;
        self.generators.push(generator);
        Ok(FilletObject::Generator(id))
    }
    #[allow(clippy::too_many_arguments)] // the native constructor's parameters
    fn allocate_constant_radius_solver(
        &mut self,
        k: &Kernel,
        edgeuse: EdgeuseId,
        radius: f64,
        approx_tol: f64,
        ang_tol_rad: f64,
        tan_tol_rad: f64,
    ) -> Result<FilletSolverId> {
        let id = FilletSolverId(
            u32::try_from(self.solvers.len()).map_err(|_| invalid("too many solvers"))?,
        );
        // Reserve the identity before construction writes a geometry pointing at it;
        // a later constructor failure must not let another solver reuse that id.
        self.solvers.push(None);
        let solver = constant_radius_from_edgeuse(
            k,
            self.target,
            &mut self.pseudo,
            id,
            approx_tol,
            ang_tol_rad,
            tan_tol_rad,
            radius,
            edgeuse,
            None,
        )?;
        *self
            .solvers
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("lost solver slot"))? =
            Some(SolverObject::ConstantRadius(Box::new(solver)));
        Ok(id)
    }
    #[allow(clippy::too_many_arguments)] // the native constructor's parameters
    fn allocate_constant_distance_solver(
        &mut self,
        k: &Kernel,
        edgeuse: EdgeuseId,
        distance: f64,
        approx_tol: f64,
        ang_tol_rad: f64,
        tan_tol_rad: f64,
    ) -> Result<FilletSolverId> {
        let id = FilletSolverId(
            u32::try_from(self.solvers.len()).map_err(|_| invalid("too many solvers"))?,
        );
        // As for the constant-radius solver: reserve the identity first.
        self.solvers.push(None);
        let solver = ConstantDistanceFs::from_edgeuse(
            k,
            self.target,
            &mut self.pseudo,
            id,
            approx_tol,
            ang_tol_rad,
            tan_tol_rad,
            distance,
            edgeuse,
        )?;
        *self
            .solvers
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("lost solver slot"))? =
            Some(SolverObject::ConstantDistance(Box::new(solver)));
        Ok(id)
    }
    fn allocate_law(&mut self, law: FilletLaw) -> Result<FilletObject> {
        let id = u32::try_from(self.laws.len()).map_err(|_| invalid("too many laws"))?;
        self.laws.push(law);
        Ok(FilletObject::Law(id))
    }
    #[allow(clippy::too_many_arguments)] // the native constructor's parameters
    fn allocate_variable_radius_solver(
        &mut self,
        k: &Kernel,
        edgeuse: EdgeuseId,
        radius: f64,
        approx_tol: f64,
        ang_tol_rad: f64,
        tan_tol_rad: f64,
        law: FilletObject,
        reverse: bool,
    ) -> Result<FilletSolverId> {
        let FilletObject::Law(law) = law else {
            return Err(invalid("object is not a law"));
        };
        let law = self
            .laws
            .get(law as usize)
            .cloned()
            .ok_or_else(|| invalid("law is not live"))?;
        let id = FilletSolverId(
            u32::try_from(self.solvers.len()).map_err(|_| invalid("too many solvers"))?,
        );
        // As for the constant-radius solver: reserve the identity first.
        self.solvers.push(None);
        let solver = VariableRadiusFs::from_edgeuse(
            k,
            self.target,
            &mut self.pseudo,
            id,
            approx_tol,
            ang_tol_rad,
            tan_tol_rad,
            radius,
            edgeuse,
            law,
            reverse,
        )?;
        *self
            .solvers
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("lost solver slot"))? =
            Some(SolverObject::VariableRadius(Box::new(solver)));
        Ok(id)
    }
    fn assign_generator(&mut self, solver: FilletSolverId, generator: FilletObject) -> Result<()> {
        let FilletObject::Generator(id) = generator else {
            return Err(invalid("object is not a generator"));
        };
        let generator = *self
            .generators
            .get(id as usize)
            .ok_or_else(|| invalid("generator is not live"))?;
        self.solver_mut(solver)?.base_mut().fsg = Some(generator);
        Ok(())
    }
}

impl FilletParametersAccess<FilletObject> for SessionObjects<'_> {
    fn parameters_tolerance(&self, k: &Kernel, brep: FilletObject, approx: bool) -> Result<f64> {
        let body = match brep {
            FilletObject::Body(FilletBody::Target) => &*self.target,
            FilletObject::Body(FilletBody::Pseudo) => self.pseudo.brep(),
            _ => return Err(invalid("object is not a session body")),
        };
        Ok(if approx {
            crate::geom::tol::approx_tol3d(k, 0.5 * body.tolerance())
        } else {
            body.tolerance()
        })
    }
    fn parameters_new_generator(
        &mut self,
        _k: &mut Kernel,
        generator: FilletSurfaceGenerator,
    ) -> Result<FilletObject> {
        self.allocate_generator(generator)
    }
    fn parameters_blend_edgeuse(
        &mut self,
        k: &mut Kernel,
        edge: FilletObject,
    ) -> Result<Option<FilletObject>> {
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)) = edge else {
            return Err(invalid("edge is not in the target"));
        };
        Ok(crate::topology::edge::blend_edgeuse(k, self.target, id)?
            .map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(id))))
    }
    fn parameters_edge_tolerance(&self, edge: FilletObject) -> Result<f64> {
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)) = edge else {
            return Err(invalid("edge is not in the target"));
        };
        crate::topology::edge::tolerance(self.target, id)
    }
    fn parameters_new_solver(
        &mut self,
        k: &mut Kernel,
        spec: &SolverSpec<'_, FilletObject>,
    ) -> Result<FilletObject> {
        let target_edgeuse = |edgeuse| match edgeuse {
            FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(edgeuse)) => Ok(edgeuse),
            _ => Err(invalid("solver edge use is not in the target")),
        };
        match *spec {
            SolverSpec::ConstantRadius {
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                radius,
                edgeuse,
            } => self.allocate_constant_radius_solver(
                k,
                target_edgeuse(edgeuse)?,
                radius,
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
            ),
            SolverSpec::ConstantDistance {
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                radius,
                edgeuse,
            } => self.allocate_constant_distance_solver(
                k,
                target_edgeuse(edgeuse)?,
                radius,
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
            ),
            SolverSpec::VariableRadius {
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                radius,
                edgeuse,
                law,
                reverse,
            } => self.allocate_variable_radius_solver(
                k,
                target_edgeuse(edgeuse)?,
                radius,
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                law,
                reverse,
            ),
            SolverSpec::SurfaceSurface { .. } => Err(invalid(
                "this session constructor supports edge-use solvers only",
            )),
        }
        .map(FilletObject::Solver)
    }
    fn parameters_set_generator(
        &mut self,
        solver: FilletObject,
        generator: FilletObject,
    ) -> Result<()> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        self.assign_generator(id, generator)
    }
}

impl SessionObjects<'_> {
    fn make_nx1_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let edgeuse = self
            .corners
            .get(id.0 as usize)
            .and_then(|corner| corner.solver_eus.as_deref())
            .and_then(|edgeuses| edgeuses.first())
            .copied()
            .ok_or_else(|| invalid("corner has no actual solver edge use"))?;
        let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(edgeuse));
        let Some(FilletObject::Solver(solver)) = executive.fillet_solver_of_edgeuse(object) else {
            return Err(SolidError::null_pointer(
                "SmFilletNx1Corner::MakeCornerTopology",
            ));
        };
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            ..
        } = self;
        let solver = solvers
            .get(solver.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("corner solver is not live"))?
            .base();
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        corner.make_nx1_corner_topology(crate::blend::fillet_corner::topology::Nx1TopologyEnv {
            k,
            target,
            pseudo,
            corner: id,
            solver,
            do_global_merge: &mut executive.options.global_merge,
        })
    }
    fn make_nx2_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let edgeuses = self
            .corners
            .get(id.0 as usize)
            .and_then(|corner| corner.solver_eus.as_deref())
            .ok_or_else(|| invalid("corner has no actual solver edge uses"))?;
        let mut solver_ids = [FilletSolverId(0); 2];
        for (slot, index) in solver_ids.iter_mut().zip(0..2) {
            let edgeuse = edgeuses
                .get(index)
                .copied()
                .ok_or_else(|| invalid("corner has fewer than two solver edge uses"))?;
            let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(edgeuse));
            let Some(FilletObject::Solver(solver)) = executive.fillet_solver_of_edgeuse(object)
            else {
                return Err(SolidError::null_pointer(
                    "SmFilletNx2Corner::MakeCornerTopology",
                ));
            };
            *slot = solver;
        }
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            ..
        } = self;
        let live = |solver: FilletSolverId| {
            solvers
                .get(solver.0 as usize)
                .and_then(Option::as_ref)
                .map(SolverObject::base)
                .ok_or_else(|| invalid("corner solver is not live"))
        };
        let [first, second] = solver_ids;
        let solvers = [live(first)?, live(second)?];
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        corner.make_nx2_corner_topology(crate::blend::fillet_corner::topology::Nx2TopologyEnv {
            k,
            target,
            pseudo,
            corner: id,
            solvers,
        })
    }
    fn make_convex_nxn_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        const NATIVE: &str = "SmFilletConvexNxNCorner::MakeCornerTopology";
        let edgeuses = self
            .corners
            .get(id.0 as usize)
            .and_then(|corner| corner.all_eus.as_deref())
            .ok_or_else(|| invalid("corner has no outward edge uses"))?;
        let lookup = |eu: EdgeuseId| {
            let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(eu));
            match executive.fillet_solver_of_edgeuse(object) {
                Some(FilletObject::Solver(solver)) => Ok(Some(solver)),
                None => Ok(None),
                Some(_) => Err(invalid("edge use does not name an actual solver")),
            }
        };
        // The last edge use's solver is looked up without the mate fallback.
        let last = edgeuses
            .last()
            .copied()
            .ok_or_else(|| invalid("corner without edge uses"))?;
        if lookup(last)?.is_none() {
            return Err(SolidError::null_pointer(NATIVE));
        }
        let mut solver_ids = Vec::new();
        for &eu in edgeuses {
            let solver = if let Some(solver) = lookup(eu)? {
                solver
            } else {
                let mate = crate::topology::edgeuse::p1::mate(self.target, eu)?
                    .ok_or_else(|| SolidError::null_pointer(NATIVE))?;
                lookup(mate)?.ok_or_else(|| SolidError::null_pointer(NATIVE))?
            };
            solver_ids.push(solver);
        }
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            ..
        } = self;
        let solvers = solver_ids
            .into_iter()
            .map(|solver| {
                solvers
                    .get(solver.0 as usize)
                    .and_then(Option::as_ref)
                    .map(SolverObject::base)
                    .ok_or_else(|| invalid("corner solver is not live"))
            })
            .collect::<Result<Vec<_>>>()?;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        corner.make_convex_nxn_corner_topology(
            crate::blend::fillet_corner::topology::NxNTopologyEnv {
                k,
                target,
                pseudo,
                corner: id,
                solvers,
            },
        )
    }
    fn calc_nx1_corner_vert_geom(&mut self, k: &Kernel, id: FilletCornerId) -> Result<()> {
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            extended_surfaces,
            ..
        } = self;
        let corner = corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        if !matches!(
            corner.corner_type(),
            FilletCornerType::NByOne | FilletCornerType::NByTwo | FilletCornerType::NByNConvex
        ) {
            return Err(invalid("ordinary Nx1, Nx2 or convex NxN corner required"));
        }
        let mut context = (
            k,
            target,
            pseudo,
            solvers,
            corners.as_slice(),
            &*extended_surfaces,
        );
        corner.calc_corner_vert_geom_with(
            &mut context,
            |(_, _, pseudo, _, _, _), fv| Ok(pseudo.fillet_vertex(fv)?.is_processed()),
            |(k, target, pseudo, solvers, corners, extended_surfaces), fv| {
                let vertex = pseudo.fillet_vertex(fv)?;
                if vertex.is_processed() {
                    return Ok(());
                }
                match vertex.fillet_vertex_type() {
                    FilletVertexType::OnVertex => pseudo.calc_vert_on_vert(fv, target),
                    FilletVertexType::RailXEdgeuse => {
                        let live = live_vertex_solvers(pseudo, solvers, fv)?;
                        pseudo.calc_rail_int_edgeuse_with_live_solvers(
                            k,
                            fv,
                            None,
                            CornerLookup::table(corners),
                            live,
                            target,
                        )
                    }
                    FilletVertexType::RailXRail => {
                        let live = live_vertex_solvers(pseudo, solvers, fv)?;
                        pseudo.calc_rail_int_rail_with_solver_ops(
                            k,
                            fv,
                            CornerLookup::table(corners),
                            live,
                            target,
                        )
                    }
                    FilletVertexType::FilletXFillet => pseudo.calc_fillet_int_fillet_with_context(
                        k,
                        fv,
                        CornerLookup::table(corners),
                        &SessionEdgeLookup {
                            solvers,
                            extended_surfaces,
                        },
                        target,
                    ),
                    FilletVertexType::FilletX2Fillets => pseudo
                        .calc_fillet_int_2_fillets_with_context(
                            k,
                            fv,
                            CornerLookup::table(corners),
                            &SessionEdgeLookup {
                                solvers,
                                extended_surfaces,
                            },
                            target,
                        ),
                    _ => {
                        // UNPORTED(blend.fillet_executive_p1): retained solver-store dispatch for other corner vertex types.
                        Err(SolidError::unported(
                            "SmFilletVertex::CalcCornerVertGeom (live solver dispatch)",
                            "blend.fillet_executive_p1",
                        ))
                    }
                }
            },
            |(_, target, pseudo, solvers, _, _), corner, processed| {
                corner.adjust_vertex_law_intervals(
                    pseudo,
                    target,
                    solvers.as_mut_slice(),
                    processed,
                )
            },
        )
    }

    fn corner_object(&self, object: FilletObject) -> Result<&FilletCorner> {
        let FilletObject::Corner(id) = object else {
            return Err(invalid("object is not a corner"));
        };
        self.corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))
    }
}

impl CornerVertexAccess<FilletObject> for SessionObjects<'_> {
    fn vertices_make_topology(
        &mut self,
        k: &mut Kernel,
        corner: FilletObject,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        match self.corner_object(corner)?.corner_type() {
            FilletCornerType::NByOne => self.make_nx1_corner_topology(k, id, executive),
            FilletCornerType::NByTwo => self.make_nx2_corner_topology(k, id, executive),
            FilletCornerType::NByNConvex => self.make_convex_nxn_corner_topology(k, id, executive),
            _ => {
                // UNPORTED(blend.fillet_executive_p1): retained context for other concrete corners.
                Err(SolidError::unported(
                    "SmFilletCorner::MakeCornerTopology (live concrete dispatch)",
                    "blend.fillet_executive_p1",
                ))
            }
        }
    }
    fn vertices_error_info(
        &self,
        corner: FilletObject,
        _executive: &FilletExecutive<FilletObject>,
    ) -> Result<CornerView<FilletObject>> {
        let corner = self.corner_object(corner)?;
        Ok(CornerView {
            vertex: corner
                .filleted_vertex()
                .map(|v| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))),
            corner_type: corner.corner_type(),
            name: corner.kind.name().into(),
        })
    }
    fn vertices_is_blending(&self, corner: FilletObject) -> Result<bool> {
        Ok(self.corner_object(corner)?.is_blending_corner())
    }
    fn vertices_is_tangent(&self, corner: FilletObject) -> Result<bool> {
        Ok(self.corner_object(corner)?.is_tangent_corner())
    }
    fn vertices_solvers(
        &self,
        corner: FilletObject,
        executive: &FilletExecutive<FilletObject>,
    ) -> Result<Vec<FilletObject>> {
        self.corner_object(corner)?
            .fillet_solvers_with_lookup(|eu| {
                match executive.fillet_solver_of_edgeuse(FilletObject::Topology(
                    FilletBody::Target,
                    EntityRef::Edgeuse(eu),
                )) {
                    Some(FilletObject::Solver(id)) => Ok(Some(id)),
                    None => Ok(None),
                    _ => Err(invalid("edge use does not name an actual solver")),
                }
            })?
            .into_iter()
            .map(|id| {
                id.map(FilletObject::Solver)
                    .ok_or_else(|| SolidError::null_pointer(OP))
            })
            .collect()
    }
    fn vertices_solver_edge(&self, solver: FilletObject) -> Result<FilletObject> {
        let eu = self
            .load_solver_edgeuse(solver, 0)?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        self.load_edgeuse_edge(eu)?
            .ok_or_else(|| SolidError::null_pointer(OP))
    }
    fn vertices_curve_ends(
        &self,
        k: &Kernel,
        edge: FilletObject,
    ) -> Result<(crate::math::Point3, crate::math::Point3)> {
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)) = edge else {
            return Err(invalid("edge is not in the target"));
        };
        let curve = self
            .target
            .edge(id)?
            .curve
            .as_ref()
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let ends = curve.ends(k)?;
        Ok((ends.start, ends.end))
    }
    fn vertices_corner_vertex(&self, corner: FilletObject) -> Result<Option<FilletObject>> {
        Ok(self
            .corner_object(corner)?
            .filleted_vertex()
            .map(|v| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))))
    }
    fn vertices_point(&self, vertex: FilletObject) -> Result<crate::math::Point3> {
        let FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(id)) = vertex else {
            return Err(invalid("vertex is not in the target"));
        };
        Ok(self.target.vertex(id)?.point)
    }
    fn vertices_approx_tolerance(&self, corner: FilletObject) -> Result<f64> {
        Ok(self.corner_object(corner)?.this_approx_tol_3d())
    }
    fn vertices_set_constant(&mut self, corner: FilletObject, value: bool) -> Result<()> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        self.corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?
            .set_calc_as_const_rad(value);
        Ok(())
    }
    fn vertices_calculate(
        &mut self,
        k: &mut Kernel,
        corner: FilletObject,
        _executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        if !matches!(
            self.corner_object(corner)?.corner_type(),
            FilletCornerType::NByOne | FilletCornerType::NByTwo | FilletCornerType::NByNConvex
        ) {
            // UNPORTED(blend.fillet_executive_p1): retained context for other concrete corners.
            return Err(SolidError::unported(
                "SmFilletCorner::CalcCornerVertGeom (live concrete dispatch)",
                "blend.fillet_executive_p1",
            ));
        }
        self.calc_nx1_corner_vert_geom(k, id)
    }
}

impl CornerAdjustmentAccess<FilletObject> for SessionObjects<'_> {
    fn adjustment_check_degeneracy(
        &mut self,
        _k: &mut Kernel,
        corner: FilletObject,
        _executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<bool> {
        let corner = self.corner_object(corner)?;
        let vertex = corner
            .filleted_vertex()
            .ok_or_else(|| invalid("corner without original vertex"))?;
        let point = self.target.vertex(vertex)?.point;
        if !corner.vertices_are_degenerate(&self.pseudo, point)? {
            return Ok(false);
        }
        // UNPORTED(blend.fillet_executive_p1): actual retained topology/attribute mutation during degeneracy fixing.
        Err(SolidError::unported(
            "SmFilletCorner::CheckAndFixDegeneracy (live mutation context)",
            "blend.fillet_executive_p1",
        ))
    }
    fn adjustment_topology(
        &mut self,
        k: &mut Kernel,
        corner: FilletObject,
        _executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        let SessionObjects {
            target,
            pseudo,
            corners,
            solvers,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let solvers = &**solvers;
        let mut context = SessionNx2Adjust {
            k,
            target,
            pseudo,
            corner: id,
        };
        corner.adjust_topology_with(
            &mut context,
            FilletCorner::adjust_topology_nx2_with,
            // SmFilletConcaveNxNCorner inherits SmFilletConvexNxNCorner::AdjustTopology.
            |corner, context| {
                corner.adjust_topology_convex_nxn_with(
                    context.k,
                    context.target,
                    context.pseudo,
                    Some(context.corner),
                    &|solver: FilletSolverId| {
                        solvers
                            .get(solver.0 as usize)
                            .and_then(Option::as_ref)
                            .map(SolverObject::base)
                            .ok_or_else(|| invalid("corner solver is not live"))
                    },
                )
            },
        )
    }
}
