//! Live object storage for one fillet executive.
//!
//! Upstream: `source/SMLib/src/SmFilletExecutive.cpp`, `source/SMLib/inc/SmFilletExecutive.h`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`FilletSession`] | `SmFilletExecutive` and its owned objects |
//! | [`SolverObject`] | the concrete `SmFilletSolver` objects |
//! | [`FilletSession::set_self_intersection_handler`] | `SetSelfIntersectionHandler` |
//! | [`FilletSession::set_curve_blend_self_intersection_handler`] | `SetSelfIntersectionHandler` (`SmMakeCurveBlendSIH`) |
//! | [`FilletSession::fillet_corners`] | `GetFilletCorners` |
//! | [`FilletSession::set_bevel_corner`] | `SetBevelCorner` |
//! | `big_radius::test_and_convert_big_rad_fillet` (private) | `SmFilletExecutive::TestAndConvertBigRadFillet` |
//! | [`FilletSession::prepare_variable_radius_edges`] | `SetFilletParameters` (same parameters on each edge) with `SM_FS_VARIABLE_RADIUS` |
//! | [`FilletSession::preview_fillet_surfaces`] | `GetPreviewFilletSurfaces` |
//!
//! Deviations: the retained handler's algorithm type is copied into every loaded
//! solver (on load and on each set), standing in for the solvers' live
//! `m_pExecutive->m_pSelfIntersectionHandler` read. One borrowed target (and, for a surface/surface fillet, an optional second base B-rep: `FilletSession::with_second_base`) and typed owned slots replace native pointers.
//! Slots are never reused. Target curves cannot be replaced while its loading caches
//! are retained: the session exposes only shared access to its borrowed target.
//! Loading creates and retains default full-tree curve caches: only their knot
//! continuities are consumed, calculated at a fixed 0.5 degrees before tessellation
//! (`SmCurveCache.cpp:1508`, `curve_cache.rs:608`). The frozen target makes repeated
//! loads equivalent. An external custom/non-full-tree cache provider is not yet
//! accepted; this is not an arbitrary warmed-cache parity claim.
//! Live corner stages dispatch `NByOne`, `NByOneClosed`, `NByTwo` and `NxN` corners
//! to the ported corner code (a closed corner's tangent split stays unported; its
//! opening checks run live); the running corner of a topology adjustment is moved
//! out of its slot for the call, so the big-radius conversion can reach the other
//! corners. One vertex dispatch (`live_corner_vert_geom`) serves every corner kind;
//! mate vertices nest through it, bounded by `MAX_MATE_DEPTH`. An Nx2 corner's
//! vertex calculators receive every retained solver of the
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
//! Attributes (issue #1462): upstream they live on the bodies' objects; here
//! [`FilletSession::with_second_base`] moves the target's (and second base's) own
//! attributes ([`Brep::add_attribute`]) into the session store under the bodies' issued
//! identities, after the executive's `m_pAttributeE`, and dropping the session
//! (`~SmFilletExecutive`: `m_pAttributeE` leaves its users and is deleted) hands each
//! body the lists of its live objects back. While the session lives the bodies' own
//! stores are empty. `take_fillet_brep` gives the fillet B-rep its objects' attributes
//! less `m_pAttributeE`.

pub mod insertion;

use std::collections::{BTreeMap, BTreeSet};

use super::p1::{CornerCreation, ExecutiveObjects, SolverLoadingAccess};
use super::p2::{EdgeMapAccess, FilletParameters, FilletParametersAccess, SolverSpec};
use super::p3::{
    BrepCreationAccess, CornerAdjustmentAccess, CornerEdgeAccess, CornerVertexAccess, CornerView,
    ErrorInfo, FaceConstructionAccess, FaceShrinkAccess, FaceStitchAccess, FilletExecutive,
    OpenEnd, RailClassificationAccess, RailTrimAccess, SolverGeometryAccess, StitchAttempt,
    VertexEntry,
};
use crate::blend::fillet_corner::p1::{EndRibDrop, LawIntervalSolvers};
use crate::blend::fillet_corner::p3::{
    CornerFilletBrep, CornerPatchEnv, CornerSolverStore, ExecutiveErrorObject, FilletCorner,
    FilletCornerKind, Nx2AdjustContext, TangentSplitEnv, TangentSplitOps,
};
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilletBrep, FilletCornerId, FilletCornerType, FilletSolverId, FilletVertexType,
};
use crate::blend::fillet_geom::p3::SplitCorners;
use crate::blend::fillet_geom::p5::FilletEdgeContext;
use crate::blend::fillet_section::section::OperationScope;
use crate::blend::fillet_solver::curve_blend_sih::MakeCurveBlendSih;
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
use crate::heal::stitch::p1::{
    select_topology_to_keep, DefaultStitchCallback, KeepChoice, StitchCallback,
};
use crate::intersect::point_class::{ClassObject, ClassObjectKind};
use crate::kernel::Kernel;
use crate::surface::surface::Surface;
use crate::topology::{Brep, EdgeId, EdgeuseId, FaceId, VertexId};

pub mod ball_regular;
pub mod band_cuts;
mod big_radius;
mod blend_face_log;
mod classification;
mod cliff_overflow;
mod concave_nxn;
mod corner_marks;
mod crossing_rails;
mod ease;
mod edge_refusals;
pub(crate) mod finish;
pub mod fit_size;
mod keep_features;
mod law_corners;
mod original_merge;
mod original_trim;
mod outline_rims;
mod overflow_first;
mod piecewise;
mod surface;
mod trace_reach;
mod trim_at_creases;
mod two_offsets;
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
    /// A surface the corner code names in an error note (`RecordFilletError`).
    CornerError(ExecutiveErrorObject),
    /// A fillet surface or centre line a solver names in an error note
    /// (`SmFilletSolver::RecordFilletError`).
    SolverError(crate::blend::fillet_solver::p2::SolverErrorObject),
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

    /// `GetSolverType` of this actual concrete solver (the derived class's override).
    ///
    /// # Errors
    /// Those of the concrete solver's [`FilletSolverOps::solver_type`].
    pub fn solver_type(&self) -> Result<FilletSolverType> {
        match self {
            Self::ConstantRadius(solver) => FilletSolverOps::solver_type(solver.as_ref()),
            Self::ConstantDistance(solver) => FilletSolverOps::solver_type(solver.as_ref()),
            Self::VariableRadius(solver) => FilletSolverOps::solver_type(solver.as_ref()),
            Self::SurfaceSurface(solver) => FilletSolverOps::solver_type(solver.as_ref()),
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

/// A surface/surface fillet's second base B-rep when its face is not in the target
/// (upstream `pBaseFace->GetBrep()` of the second rail), with its issued identity and
/// the intersector its rails merge through (see `session::surface`).
#[derive(Debug)]
struct SecondBase<'target> {
    body: &'target mut Brep,
    serial: u64,
    intersector:
        Option<crate::boolean::topology_intersector::p1::TopologyIntersector<FilletObject>>,
}

/// Owned objects and the target borrowed by the executive.
#[derive(Debug)]
struct SessionObjects<'target> {
    target: &'target mut Brep,
    second_base: Option<SecondBase<'target>>,
    pseudo: FilletBrep,
    intermediate: Option<Brep>,
    insertion_curves: insertion::curves::InsertionCurves,
    notification_records: crate::topology::topology::NotifyRecords,
    solvers: Vec<Option<SolverObject>>,
    corners: Vec<FilletCorner>,
    generators: Vec<FilletSurfaceGenerator>,
    laws: Vec<FilletLaw>,
    attributes: AttributeStore,
    /// (not upstream) The target's and the second base's own attribute stores as the
    /// session took them, put back by `Drop` if handing the session's back fails.
    attributes_before: (AttributeStore, AttributeStore),
    merge_attributes: Option<Vec<AttributeHandle>>,
    /// WR-459: the target's edges and faces before filleting, kept by the clean-up.
    cleanup_kept: Option<(BTreeSet<EdgeId>, BTreeSet<FaceId>)>,
    extended_surfaces: BTreeMap<FaceId, Surface>,
    edge_caches: BTreeMap<EdgeId, CurveCache<FilletObject>>,
    target_serial: u64,
    self_intersection_handler: Option<MakeSurfaceBlendSih>,
    curve_blend_self_intersection_handler: Option<MakeCurveBlendSih>,
    /// The fork's `m_bSweptTurns` (`SetSweptTurns`; WR-697): a blend corner's Turn is
    /// Swept, not Mitred. Read only with `Kernel::fixes.fillet_blend_corner`.
    swept_turns: bool,
    /// The fork's `m_vSetBackVertices` (`AddSetBackVertex`; WR-697, 79d4763): the
    /// vertices the caller sets back, where no Ball is built. Read only with
    /// `Kernel::fixes.fillet_blend_corner`.
    set_back_vertices: Vec<VertexId>,
    /// The fork's `m_vSetBackFills` (`SetSetBackFill`; smlib#104 61547af): the corners
    /// asked to be set back, before the corners are made. Read only with
    /// `Kernel::fixes.fillet_blend_corner`.
    set_back_fills: Vec<crate::blend::blend_corner::setback::SetBackFill>,
    /// The fork's `m_vRadiusCornerSurfaces` and `m_vRadiusCornerPieces` (6eecf04, 58d03ef):
    /// the fillet surfaces noted before the merge where their radius law turns, which the
    /// split after it finds again ([`law_corners`]).
    radius_corner_surfaces: Vec<law_corners::RadiusCornerSurface>,
    /// (not upstream) The last Setback refusal noted, as data, for `DoFilleting` to return
    /// once its later steps have made it their own failure (`edge_refusals`).
    setback_refusal: Option<crate::blend::blend_refusal::BlendRefusal>,
    /// (not upstream, #2489) The marks of the faces the session made, while
    /// `Kernel::blend_face_log` is on (`blend_face_log`).
    blend_face_marks: Vec<AttributeHandle>,
    /// The corner type of chamfers meeting at a vertex (`set_chamfer_corner`; WR-716).
    /// Read only with `Kernel::fixes.fillet_chamfer_corner`.
    chamfer_corner: crate::blend::blend_corner::BlendChamferCorner,
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
        use crate::blend::fillet_geom::p1::{FilletSurfaceOwner, FilletSurfaceStorage};
        let geometry = self.pseudo.geom(geom)?;
        // Before installation no intermediate body may exist yet.
        let owner = match geometry.fillet_surface {
            Some(FilletSurfaceStorage::ManagedFace { .. }) => Some(FilletSurfaceOwner {
                identity: self.intermediate_identity(EntityRef::Brep(
                    crate::topology::BrepId::from_index(0),
                ))?,
                body: self
                    .intermediate
                    .as_ref()
                    .ok_or_else(|| invalid("managed surface body is not allocated"))?,
            }),
            _ => None,
        };
        geometry.fillet_surface(owner)
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
    /// `pFilletSolver->CalcFilletGeom()`; the solver's `RecordFilletError` notes are
    /// replayed into the executive's record (`m_pExecutive->NoteFilletError`).
    fn calc_solver_geometry(
        &mut self,
        k: &Kernel,
        id: FilletSolverId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let mut note = ErrorInfo::default();
        let result = self.calc_solver_geometry_noting(k, id, executive, &mut note);
        replay_solver_error(&mut executive.error, &note);
        result
    }

    fn calc_solver_geometry_noting(
        &mut self,
        k: &Kernel,
        id: FilletSolverId,
        executive: &FilletExecutive<FilletObject>,
        note: &mut ErrorInfo<crate::blend::fillet_solver::p2::SolverErrorObject>,
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
        let mut context = SessionGeometryContext {
            current: id,
            before,
            after,
            executive,
            corners: &mut self.corners,
        };
        match current {
            SolverObject::ConstantRadius(solver) => FilletSolver::calc_fillet_geom_with_context(
                solver.as_mut(),
                k,
                self.target,
                &mut self.pseudo,
                &mut context,
                note,
            ),
            // `SmVariableRadiusFS` inherits `SmFilletSolver::CalcFilletGeom`.
            SolverObject::VariableRadius(solver) => FilletSolver::calc_fillet_geom_with_context(
                solver.as_mut(),
                k,
                self.target,
                &mut self.pseudo,
                &mut context,
                note,
            ),
            // `SmConstantDistanceFS` inherits `SmFilletSolver::CalcFilletGeom`.
            SolverObject::ConstantDistance(solver) => FilletSolver::calc_fillet_geom_with_context(
                solver.as_mut(),
                k,
                self.target,
                &mut self.pseudo,
                &mut context,
                note,
            ),
            // SmSurfaceSurfaceFS::CalcFilletGeom
            SolverObject::SurfaceSurface(solver) => solver.calc_fillet_geom_with_context(
                k,
                &mut crate::blend::fillet_standard_solver::surface_surface::BaseBodies {
                    target: self.target,
                    second: self.second_base.as_mut().map(|base| &mut *base.body),
                },
                &mut self.pseudo,
                &mut surface::SurfaceSurfaceSession {
                    corners: &self.corners,
                },
            ),
        }
    }
}

/// The retained virtual operations of a concrete solver.
fn live_solver_ops(solver: &mut SolverObject) -> &mut dyn FilletSolverOps {
    match solver {
        SolverObject::ConstantRadius(solver) => solver.as_mut(),
        SolverObject::VariableRadius(solver) => solver.as_mut(),
        SolverObject::ConstantDistance(solver) => solver.as_mut(),
        SolverObject::SurfaceSurface(solver) => solver.as_mut(),
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

/// The session's solver slots, and the executive's edge-use map where it is at hand, as
/// the blend corner's Swept turn reads them (WR-697).
struct SweptSessionSolvers<'s> {
    solvers: &'s [Option<SolverObject>],
    executive: Option<&'s FilletExecutive<FilletObject>>,
}

impl SweptSessionSolvers<'_> {
    fn live(&self, eu: EdgeuseId) -> Result<&SolverObject> {
        let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(eu));
        let Some(FilletObject::Solver(id)) = self
            .executive
            .and_then(|executive| executive.fillet_solver_of_edgeuse(object))
        else {
            return Err(SolidError::null_pointer("SmBlendCorner::Classify"));
        };
        self.solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("swept turn solver is not live"))
    }
}

impl crate::blend::blend_corner::swept::SweptSolvers for SweptSessionSolvers<'_> {
    fn solver_of_edgeuse(&self, eu: EdgeuseId) -> Result<&FilletSolver> {
        Ok(self.live(eu)?.base())
    }
    fn offset_radii_can_change(&self, eu: EdgeuseId) -> Result<bool> {
        match self.live(eu)? {
            SolverObject::ConstantRadius(s) => FilletSolverOps::offset_radii_can_change(&**s),
            SolverObject::VariableRadius(s) => FilletSolverOps::offset_radii_can_change(&**s),
            SolverObject::ConstantDistance(s) => FilletSolverOps::offset_radii_can_change(&**s),
            SolverObject::SurfaceSurface(s) => FilletSolverOps::offset_radii_can_change(&**s),
        }
    }
    fn solver(&self, id: FilletSolverId) -> Result<&FilletSolver> {
        self.solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .map(SolverObject::base)
            .ok_or_else(|| invalid("swept turn solver is not live"))
    }
}

impl SweptSessionSolvers<'_> {
    fn slot(&self, id: FilletSolverId) -> Result<&SolverObject> {
        self.solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("ball solver is not live"))
    }
}

impl crate::blend::blend_corner::ball::BallSolvers for SweptSessionSolvers<'_> {
    fn solver_of_edgeuse(&self, eu: EdgeuseId) -> Result<Option<FilletSolverId>> {
        let executive = self
            .executive
            .ok_or_else(|| invalid("ball solver lookup without the executive"))?;
        let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(eu));
        match executive.fillet_solver_of_edgeuse(object) {
            Some(FilletObject::Solver(id)) => Ok(Some(id)),
            None => Ok(None),
            Some(_) => Err(invalid("edge use does not name an actual solver")),
        }
    }
    fn solver(&self, id: FilletSolverId) -> Result<&FilletSolver> {
        Ok(self.slot(id)?.base())
    }
    fn solver_type(&self, id: FilletSolverId) -> Result<FilletSolverType> {
        self.slot(id)?.solver_type()
    }
    fn offset_radii_can_change(&self, id: FilletSolverId) -> Result<bool> {
        match self.slot(id)? {
            SolverObject::ConstantRadius(s) => FilletSolverOps::offset_radii_can_change(&**s),
            SolverObject::VariableRadius(s) => FilletSolverOps::offset_radii_can_change(&**s),
            SolverObject::ConstantDistance(s) => FilletSolverOps::offset_radii_can_change(&**s),
            SolverObject::SurfaceSurface(s) => FilletSolverOps::offset_radii_can_change(&**s),
        }
    }
}

/// The live objects a Setback's fill reaches: the solvers as [`SweptSessionSolvers`] reads
/// them, and (to grow its setbacks beside curved faces, solidcore#2343) the concrete solvers'
/// `CalcSetBackVert` of its setback vertices.
struct SetbackSessionSolvers<'s> {
    solvers: &'s mut [Option<SolverObject>],
    executive: &'s FilletExecutive<FilletObject>,
    corner: FilletCornerId,
}

impl SetbackSessionSolvers<'_> {
    fn lookup(&self) -> SweptSessionSolvers<'_> {
        SweptSessionSolvers {
            solvers: &*self.solvers,
            executive: Some(self.executive),
        }
    }
}

impl crate::blend::blend_corner::ball::BallSolvers for SetbackSessionSolvers<'_> {
    fn solver_of_edgeuse(&self, eu: EdgeuseId) -> Result<Option<FilletSolverId>> {
        crate::blend::blend_corner::ball::BallSolvers::solver_of_edgeuse(&self.lookup(), eu)
    }
    fn solver(&self, id: FilletSolverId) -> Result<&FilletSolver> {
        self.solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .map(SolverObject::base)
            .ok_or_else(|| invalid("setback solver is not live"))
    }
    fn solver_type(&self, id: FilletSolverId) -> Result<FilletSolverType> {
        crate::blend::blend_corner::ball::BallSolvers::solver_type(&self.lookup(), id)
    }
    fn offset_radii_can_change(&self, id: FilletSolverId) -> Result<bool> {
        crate::blend::blend_corner::ball::BallSolvers::offset_radii_can_change(&self.lookup(), id)
    }
}

impl crate::blend::blend_corner::setback::SetbackSolvers for SetbackSessionSolvers<'_> {
    fn calc_set_back_vert(
        &mut self,
        k: &Kernel,
        target: &Brep,
        pseudo: &mut FilletBrep,
        corner: &FilletCorner,
        fv: VertexId,
    ) -> Result<()> {
        let live = live_vertex_solvers(pseudo, self.solvers, fv)?;
        pseudo.calc_set_back_vert_with_live_solvers(
            k,
            fv,
            CornerLookup {
                this: Some((Some(self.corner), corner)),
                ..CornerLookup::default()
            },
            live,
            target,
        )
    }
}

impl crate::blend::blend_corner::chamfer::ChamferSolvers for SweptSessionSolvers<'_> {
    fn chamfer_solver(
        &self,
        b: &Brep,
        eu: EdgeuseId,
    ) -> Result<
        Option<(
            &FilletSolver,
            Option<crate::blend::blend_corner::chamfer::ChamferSizeKind>,
        )>,
    > {
        use crate::blend::blend_corner::chamfer::ChamferSizeKind;
        let executive = self
            .executive
            .ok_or_else(|| invalid("chamfer solver lookup without the executive"))?;
        let of = |eu: EdgeuseId| {
            let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(eu));
            match executive.fillet_solver_of_edgeuse(object) {
                Some(FilletObject::Solver(id)) => Some(id),
                _ => None,
            }
        };
        let id = match of(eu) {
            Some(id) => Some(id),
            None => crate::topology::edgeuse::p1::mate(b, eu)?.and_then(of),
        };
        let Some(id) = id else {
            return Ok(None);
        };
        let slot = self
            .solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("chamfer solver is not live"))?;
        let size = match slot {
            SolverObject::ConstantRadius(s) if !FilletSolverOps::offset_radii_can_change(&**s)? => {
                Some(ChamferSizeKind::Offsets)
            }
            SolverObject::ConstantDistance(s) => Some(ChamferSizeKind::Chord(s.distance())),
            _ => None,
        };
        Ok(Some((slot.base(), size)))
    }
}

/// The fork's Setback noting why it refused at its vertex (`SetBackNotBuilt`, its
/// `NoteError(m_cpVertex, SM_FIL_FAILURE, ..)`), and the setback that builds
/// (`NoteFitSetBack`) where it names one.
fn note_setback_refusal(
    executive: &mut FilletExecutive<FilletObject>,
    target: &Brep,
    vertex: Option<VertexId>,
    refusal: &crate::blend::blend_corner::setback::SetbackRefusal,
) {
    if let Some(fit) = refusal.fit_setback {
        executive.fit_setback = fit;
    }
    let location = vertex.and_then(|v| crate::topology::vertex::point(target, v).ok());
    executive.error.note_vertex_error(
        vertex.map(|v| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))),
        location,
        crate::blend::fillet_geom::p1::FilStatus::Failure,
        refusal.error,
        Some(&refusal.comment),
    );
}

/// The error of a refused Setback: its refusal as data where it has one
/// ([`SolidError::BlendRefused`]), else `SM_ERR`; the status is `SM_ERR` either way.
fn setback_refused(
    operation: &'static str,
    refusal: &crate::blend::blend_corner::setback::SetbackRefusal,
) -> SolidError {
    match refusal.refusal {
        Some(typed) => typed.error(operation, crate::error::SM_ERR),
        None => SolidError::sm_err(operation),
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
    fn solver_type(&self, index: usize) -> Option<Result<FilletSolverType>> {
        self.0
            .get(index)
            .and_then(Option::as_ref)
            .map(SolverObject::solver_type)
    }
}

/// `SmFilletVertex::CalcCornerVertGeom` over the live session's objects: dispatches an
/// unprocessed corner vertex to its calculator by type. `depth` counts the mate
/// vertices whose main vertex is being computed (bounded by
/// [`crate::blend::fillet_geom::p3::MAX_MATE_DEPTH`]).
#[allow(clippy::too_many_arguments)] // the session's borrowed object stores
fn live_corner_vert_geom(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &mut FilletBrep,
    solvers: &mut [Option<SolverObject>],
    corners: &[FilletCorner],
    extended_surfaces: &BTreeMap<FaceId, Surface>,
    fv: VertexId,
    depth: usize,
) -> Result<()> {
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
        FilletVertexType::RailXExtendedEdgeuse => pseudo
            .calc_rail_int_extended_edgeuse_with_context(
                k,
                fv,
                CornerLookup::table(corners),
                &SessionEdgeLookup {
                    solvers,
                    extended_surfaces,
                },
                target,
            ),
        FilletVertexType::FilletX2Fillets => pseudo.calc_fillet_int_2_fillets_with_context(
            k,
            fv,
            CornerLookup::table(corners),
            &SessionEdgeLookup {
                solvers,
                extended_surfaces,
            },
            target,
        ),
        FilletVertexType::Mate => pseudo.calc_mate_geom_with(
            k,
            fv,
            target,
            solvers,
            depth,
            |pseudo, target, solvers, main| {
                live_corner_vert_geom(
                    k,
                    target,
                    pseudo,
                    solvers,
                    corners,
                    extended_surfaces,
                    main,
                    depth + 1,
                )
            },
            |solvers, target, solver, side| {
                solvers
                    .get(solver.0 as usize)
                    .and_then(Option::as_ref)
                    .ok_or_else(|| invalid("mate vertex solver is not live"))?
                    .base()
                    .find_index_of_rail_x_side_edgeuse(target, side)
            },
        ),
        FilletVertexType::OnCrossSection => {
            let live = live_vertex_solvers(pseudo, solvers, fv)?;
            pseudo.calc_vert_on_cross_section_with_live_solvers(
                k,
                fv,
                None,
                CornerLookup::table(corners),
                live,
                target,
            )
        }
        FilletVertexType::Setback => {
            let live = live_vertex_solvers(pseudo, solvers, fv)?;
            pseudo.calc_set_back_vert_with_live_solvers(
                k,
                fv,
                CornerLookup::table(corners),
                live,
                target,
            )
        }
        // the fork's SM_FV_JOIN_SECTION (543d25a; WR-697)
        FilletVertexType::JoinSection => {
            let live = live_vertex_solvers(pseudo, solvers, fv)?;
            pseudo.calc_join_section_with_live_solvers(
                k,
                fv,
                CornerLookup::table(corners),
                live,
                target,
            )
        }
        // SM_FV_FILLET_X_EDGEUSE and the rest: nothing to calculate.
        FilletVertexType::FilletXEdgeuse
        | FilletVertexType::CliffRailXEdge
        | FilletVertexType::RailEnd
        | FilletVertexType::RailXVertex
        | FilletVertexType::Unknown
        // the fork's SM_FV_PRECOMPUTED (61547af): placed by its corner's own build
        | FilletVertexType::Precomputed => Ok(()),
    }
}

/// `SmFilletCorner::CalcCornerVertGeom` of the ordinary Nx1, closed Nx1, Nx2 and `NxN`
/// corner `id` over the live session's objects (the corner sits in its slot of
/// `corners`). Not upstream (#2406, `fixes.fillet_blend_corner`): at an End that can wait
/// for its rails' rollover (`rolled_end`), a rail end its rail does not reach on the End's
/// plane takes the rail's traced end for now
/// ([`crate::blend::blend_corner::end::place_rolled_rail_end`]).
#[allow(clippy::too_many_arguments)] // the session's borrowed object stores
fn live_corner_vertices_geom(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &mut FilletBrep,
    solvers: &mut [Option<SolverObject>],
    corners: &[FilletCorner],
    extended_surfaces: &BTreeMap<FaceId, Surface>,
    id: FilletCornerId,
    rolled_end: bool,
) -> Result<()> {
    let corner = corners
        .get(id.0 as usize)
        .ok_or_else(|| invalid("corner is not live"))?;
    let mut context = (
        k,
        &mut *target,
        &mut *pseudo,
        &mut *solvers,
        corners,
        extended_surfaces,
    );
    corner.calc_corner_vert_geom_with(
        &mut context,
        |(_, _, pseudo, _, _, _), fv| Ok(pseudo.fillet_vertex(fv)?.is_processed()),
        |(k, target, pseudo, solvers, corners, extended_surfaces), fv| match live_corner_vert_geom(
            k,
            target,
            pseudo,
            solvers,
            corners,
            extended_surfaces,
            fv,
            0,
        ) {
            Err(e) if rolled_end && !e.is_unported_or_cancelled() => {
                if crate::blend::blend_corner::end::place_rolled_rail_end(
                    k, target, pseudo, corner, fv,
                )? {
                    Ok(())
                } else {
                    Err(e)
                }
            }
            result => result,
        },
        |(_, target, pseudo, solvers, _, _), corner, processed| {
            corner.adjust_vertex_law_intervals(pseudo, target, &mut **solvers, processed)
        },
    )?;
    // The fork's (WR-697, 4241a1b): SmBlendCorner::CalcCornerVertGeom places a Ball's
    // contacts in closed form after the base's vertex geometry (PlaceBallContacts)
    if corner
        .blend
        .is_some_and(crate::blend::blend_corner::BlendCorner::is_ball)
    {
        let lookup = SweptSessionSolvers {
            solvers: &*solvers,
            executive: None,
        };
        crate::blend::blend_corner::ball::place_ball_contacts(k, target, pseudo, corner, &lookup)?;
    }
    // WR-716: the chamfer event's vertices at their exact points
    if corner
        .blend
        .is_some_and(crate::blend::blend_corner::BlendCorner::is_chamfer)
    {
        crate::blend::blend_corner::chamfer::place_chamfer_vertices(pseudo, corner)?;
    }
    // The fork's (WR-697, 6fd0411): a Mixed corner places its vertices A, P, Q and B once
    // both blends are traced (CalcMixedVertGeom), after the base's vertex geometry
    if corner
        .blend
        .is_some_and(crate::blend::blend_corner::BlendCorner::is_mixed)
    {
        let lookup = SweptSessionSolvers {
            solvers: &*solvers,
            executive: None,
        };
        crate::blend::blend_corner::mixed::calc_mixed_vert_geom(
            k, target, pseudo, corner, &lookup,
        )?;
    }
    Ok(())
}

/// The live executive's `GetFilletCornerOfVertex` (`m_vVertexCornerMap`) and corner
/// table, with the running corner (out of its slot) when one is, as
/// [`SplitCorners`].
pub(super) struct SessionSplitCorners<'a> {
    pub(super) executive: &'a FilletExecutive<FilletObject>,
    pub(super) corners: &'a mut [FilletCorner],
    pub(super) this: Option<(FilletCornerId, &'a mut FilletCorner)>,
}

/// `GetFilletCornerOfVertex(vertex)` of the live executive (`None`: `NULL`).
pub(super) fn session_corner_of_vertex(
    executive: &FilletExecutive<FilletObject>,
    vertex: VertexId,
) -> Option<FilletCornerId> {
    let key = Some(FilletObject::Topology(
        FilletBody::Target,
        EntityRef::Vertex(vertex),
    ));
    match executive.vertex_corner_map.get(&key) {
        Some(VertexEntry::Corner(FilletObject::Corner(id))) => Some(*id),
        _ => None,
    }
}

impl SplitCorners for SessionSplitCorners<'_> {
    fn corner_of_vertex(&self, vertex: VertexId) -> Option<FilletCornerId> {
        session_corner_of_vertex(self.executive, vertex)
    }
    fn lookup(&self) -> CornerLookup<'_> {
        CornerLookup {
            table: Some(&*self.corners),
            this: self.this.as_ref().map(|(id, c)| (Some(*id), &**c)),
            exec: None,
        }
    }
    fn corner_mut(&mut self, id: FilletCornerId) -> Result<&mut FilletCorner> {
        if let Some((this_id, this)) = self.this.as_mut() {
            if *this_id == id {
                return Ok(this);
            }
        }
        self.corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| SolidError::null_pointer(OP))
    }
}

/// The live objects the tangent-corner splits reach (`AdjustTangentCorner`): the
/// session's concrete solvers and corner table, the running corner's slot holding a
/// placeholder.
struct SessionTangentSplit<'a> {
    executive: &'a FilletExecutive<FilletObject>,
    solvers: &'a mut [Option<SolverObject>],
    corners: &'a mut Vec<FilletCorner>,
    extended_surfaces: &'a BTreeMap<FaceId, Surface>,
    id: FilletCornerId,
}

impl SessionTangentSplit<'_> {
    fn slot(&mut self, id: FilletSolverId) -> Result<&mut SolverObject> {
        self.solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("fillet geometry solver is not live"))
    }
}

impl TangentSplitOps for SessionTangentSplit<'_> {
    fn solver(&self, id: FilletSolverId) -> Result<&FilletSolver> {
        self.solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .map(SolverObject::base)
            .ok_or_else(|| invalid("fillet geometry solver is not live"))
    }
    fn solver_ops(&mut self, id: FilletSolverId) -> Result<&mut dyn FilletSolverOps> {
        Ok(live_solver_ops(self.slot(id)?))
    }
    fn split_fillet_geom_at_side_edge(
        &mut self,
        k: &Kernel,
        target: &mut Brep,
        pseudo: &mut FilletBrep,
        corner: &mut FilletCorner,
        id: FilletSolverId,
        fillet_geom: crate::blend::fillet_geom::p1::FilletGeomId,
        split_vertexuse: crate::topology::VertexuseId,
        which_end: usize,
    ) -> Result<crate::blend::fillet_geom::p1::FilletGeomId> {
        let this = self.id;
        let executive = self.executive;
        let solver = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("fillet geometry solver is not live"))?;
        solver.base_mut().split_fillet_geom_at_side_edge(
            k,
            target,
            pseudo,
            fillet_geom,
            split_vertexuse,
            which_end,
            &mut SessionSplitCorners {
                executive,
                corners: self.corners.as_mut_slice(),
                this: Some((this, corner)),
            },
        )
    }
    fn calc_corner_vert_geom(
        &mut self,
        k: &Kernel,
        target: &mut Brep,
        pseudo: &mut FilletBrep,
        corner: &mut FilletCorner,
        _corner_id: Option<FilletCornerId>,
    ) -> Result<()> {
        // The corner goes back into its slot for the calculation (the vertex
        // calculators find it through the corner table).
        let index = self.id.0 as usize;
        let slot = self
            .corners
            .get_mut(index)
            .ok_or_else(|| invalid("corner is not live"))?;
        std::mem::swap(slot, corner);
        let result = live_corner_vertices_geom(
            k,
            target,
            pseudo,
            self.solvers,
            self.corners,
            self.extended_surfaces,
            self.id,
            false,
        );
        if let Some(slot) = self.corners.get_mut(index) {
            std::mem::swap(slot, corner);
        }
        result
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
        live.push((id, live_solver_ops(solver)));
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
    solvers: &'a mut [Option<SolverObject>],
    /// The corner slots, the running corner's holding a placeholder.
    corners: &'a mut Vec<FilletCorner>,
    /// The executive's `m_vExtendedSurfacesMap`.
    extended_surfaces: &'a mut BTreeMap<FaceId, Surface>,
    executive: &'a mut FilletExecutive<FilletObject>,
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
    fn adjust_concave_corner_topology(&mut self, corner: &mut FilletCorner) -> Result<()> {
        corner.adjust_concave_corner_topology_with(
            self.k,
            self.target,
            self.pseudo,
            Some(self.corner),
            self.extended_surfaces,
        )
    }
    fn test_and_convert_big_rad_fillet(
        &mut self,
        corner: &mut FilletCorner,
        fg: crate::blend::fillet_geom::p1::FilletGeomId,
    ) -> Result<bool> {
        big_radius::test_and_convert_big_rad_fillet(
            &mut big_radius::BigRadEnv {
                k: self.k,
                target: &mut *self.target,
                pseudo: &mut *self.pseudo,
                solvers: &mut *self.solvers,
                corners: &mut *self.corners,
                executive: &mut *self.executive,
            },
            self.corner,
            corner,
            fg,
        )
    }
}

struct SessionGeometryContext<'a> {
    current: FilletSolverId,
    before: &'a [Option<SolverObject>],
    after: &'a [Option<SolverObject>],
    executive: &'a FilletExecutive<FilletObject>,
    corners: &'a mut [FilletCorner],
}

impl crate::blend::fillet_solver::p1::ExtensionCorners for SessionGeometryContext<'_> {
    fn corner_of(&self, vertex: Option<VertexId>) -> Result<&FilletCorner> {
        self.corner(vertex)
    }
}

impl SplitCorners for SessionGeometryContext<'_> {
    fn corner_of_vertex(&self, vertex: VertexId) -> Option<FilletCornerId> {
        session_corner_of_vertex(self.executive, vertex)
    }
    fn lookup(&self) -> CornerLookup<'_> {
        CornerLookup::table(self.corners)
    }
    fn corner_mut(&mut self, id: FilletCornerId) -> Result<&mut FilletCorner> {
        self.corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| SolidError::null_pointer(OP))
    }
}

impl SessionGeometryContext<'_> {
    fn corner(&self, vertex: Option<VertexId>) -> Result<&FilletCorner> {
        let key =
            vertex.map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(id)));
        let Some(VertexEntry::Corner(FilletObject::Corner(id))) =
            self.executive.vertex_corner_map.get(&key)
        else {
            // upstream dereferences the NULL corner
            return Err(SolidError::null_pointer(OP));
        };
        self.corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live in this session"))
    }
}

impl FilletGeometryContext for SessionGeometryContext<'_> {
    fn set_extension_flags(
        &self,
        solver: &mut dyn FilletSolverOps,
        k: &Kernel,
        target: &Brep,
    ) -> Result<()> {
        // the virtual SetExtensionFlags (SmVariableRadiusFS extends its edge curve)
        solver.set_extension_flags(k, target, self)
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

    fn corners(&self) -> CornerLookup<'_> {
        CornerLookup::table(self.corners)
    }

    fn split_corners(&mut self) -> &mut dyn SplitCorners {
        self
    }
}

/// A corner's face-error note (`FILEXEC_CORNER_ERR`).
type CornerNote<'n> = &'n mut dyn FnMut(&FilletCorner);

struct SessionEdgeLookup<'a> {
    solvers: &'a mut [Option<SolverObject>],
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
    fn solver_pair_ops_mut(
        &mut self,
        id1: FilletSolverId,
        id2: FilletSolverId,
    ) -> Result<(&mut dyn FilletSolverOps, &mut dyn FilletSolverOps)> {
        let (i, j) = (id1.0 as usize, id2.0 as usize);
        if i == j {
            return Err(invalid("one solver for both rails of a rail/rail solve"));
        }
        let (lo, hi) = if i < j { (i, j) } else { (j, i) };
        let (head, tail) = self.solvers.split_at_mut(hi);
        let not_live = || invalid("solver is not live in this session");
        let a = head
            .get_mut(lo)
            .and_then(Option::as_mut)
            .ok_or_else(not_live)?;
        let b = tail
            .first_mut()
            .and_then(Option::as_mut)
            .ok_or_else(not_live)?;
        let (a, b) = (live_solver_ops(a), live_solver_ops(b));
        Ok(if i < j { (a, b) } else { (b, a) })
    }
}

struct SessionCornerEdgeContext<'a> {
    k: &'a mut Kernel,
    target: &'a mut Brep,
    pseudo: &'a mut FilletBrep,
    lookup: SessionEdgeLookup<'a>,
    corner: FilletCornerId,
    do_global_merge: bool,
    /// The executive's `m_sErrorInfo`, for `SmFilletCorner::RecordFilletError`.
    error: &'a mut ErrorInfo<FilletObject>,
}

/// The executive identity of an object the corner code records in an error note: the
/// target and fillet vertices are their topology; the other surfaces keep the corner
/// code's identity.
fn executive_error_object(object: ExecutiveErrorObject) -> FilletObject {
    match object {
        ExecutiveErrorObject::TargetVertex(v) => {
            FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))
        }
        ExecutiveErrorObject::FilletVertex(v) => {
            FilletObject::Topology(FilletBody::Pseudo, EntityRef::Vertex(v))
        }
        other => FilletObject::CornerError(other),
    }
}

/// The executive identity of an object a solver records in an error note: the
/// filleted edge and the rails are their topology; the fillet surfaces and centre
/// lines keep the solver code's identity.
fn executive_solver_error_object(
    object: crate::blend::fillet_solver::p2::SolverErrorObject,
) -> FilletObject {
    use crate::blend::fillet_solver::p2::SolverErrorObject;
    match object {
        SolverErrorObject::FilletedEdge(e) => {
            FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e))
        }
        SolverErrorObject::FilletEdge(e) => {
            FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edge(e))
        }
        other => FilletObject::SolverError(other),
    }
}

/// Replay the solver's error notes (`SmFilletErrorInfo::NoteError` with the
/// intersection overload, made into a fresh record) into the executive's record: the
/// same field writes, with the surfaces, curves and edges added uniquely to the ones
/// already noted.
fn replay_solver_error(
    error: &mut ErrorInfo<FilletObject>,
    note: &ErrorInfo<crate::blend::fillet_solver::p2::SolverErrorObject>,
) {
    if *note == ErrorInfo::default() {
        return;
    }
    let map = |o: Option<crate::blend::fillet_solver::p2::SolverErrorObject>| {
        o.map(executive_solver_error_object)
    };
    error.code = note.code;
    error.status = note.status;
    error.edge_type = note.edge_type;
    error.edge = map(note.edge);
    for (from, to) in [
        (&note.surfaces, &mut error.surfaces),
        (&note.curves, &mut error.curves),
        (&note.edges, &mut error.edges),
    ] {
        for object in from {
            let object = map(*object);
            if !to.contains(&object) {
                to.push(object);
            }
        }
    }
    error.location = note.location;
    error.comment.clone_from(&note.comment);
    // the typed reason of a `NoRoom` the solver noted (the fork's circular section's
    // fold of its balls, WR-685)
    if note.no_room.is_some() {
        error.no_room = note.no_room;
    }
}

/// Replay one corner error note (`SmFilletErrorInfo::NoteError` with the corner
/// overload, made into a fresh record) into the executive's record: the same field
/// writes, with the surfaces added uniquely to the ones already noted.
fn replay_corner_error(
    error: &mut ErrorInfo<FilletObject>,
    note: &ErrorInfo<ExecutiveErrorObject>,
) {
    if *note == ErrorInfo::default() {
        return;
    }
    let map = |o: Option<ExecutiveErrorObject>| o.map(executive_error_object);
    error.code = note.code;
    error.status = note.status;
    error.corner_type = note.corner_type;
    error.vertex = map(note.vertex);
    for surface in &note.surfaces {
        let surface = map(*surface);
        if !error.surfaces.contains(&surface) {
            error.surfaces.push(surface);
        }
    }
    error.fillet_vertices = note.fillet_vertices.map(map);
    error.comment.clone_from(&note.comment);
    error.entity_name.clone_from(&note.entity_name);
}

/// One actual executive and its live target, pseudo-B-rep, solvers and corners.
#[derive(Debug)]
pub struct FilletSession<'target> {
    executive: FilletExecutive<FilletObject>,
    objects: SessionObjects<'target>,
    /// The fork's `SmFilletExecutive::m_sGnScope` (b8ec238, smlib#109): the blend
    /// operation's scope, for the kernel's memo of equal area thumbweights. Opened first
    /// and declared last, so it ends after the session's own teardown.
    _gn_scope: OperationScope,
}

/// (not upstream) Copies `body`'s own attributes ([`Brep::add_attribute`]) into the
/// session store under the body's issued identities (`serial`). The body keeps its
/// store: the caller empties it only once every body is seeded, so a failed seed leaves
/// each body's attributes where they were.
fn seed_body_attributes(attributes: &mut AttributeStore, body: &Brep, serial: u64) -> Result<()> {
    attributes.absorb(body.attribute_store().clone(), &|key| {
        crate::topology::loopuse::entity_of_notify_key(key)
            .map(|e| crate::topology::brep::p3::topology_ref(serial, e).key)
            .ok_or_else(|| invalid("attribute list names no object"))
    })
}

/// (not upstream) The session store's attributes of the live objects of `body`
/// (issued `serial`), keyed by the body's own identities; the lists of every other
/// object are cleared, as upstream's object destructor does.
fn body_attributes(
    attributes: &AttributeStore,
    body: &Brep,
    serial: u64,
) -> Result<AttributeStore> {
    let mut store = attributes.clone();
    store.retain_rekeyed(&|key| {
        crate::topology::brep::p3::topology_entity(serial, key)
            .filter(|&e| body.is_live_entity(e))
            .map(|e| crate::topology::loopuse::notify_ref(e).key)
    })?;
    // Only this body's attributes: those no object of it lists (the other body's, the
    // session's own) are not carried into its store.
    store.retain_listed();
    Ok(store)
}

/// (not upstream) Whether `a` is an application attribute (`SM_AI_USER_1` and up), not
/// one of the kernel's own ids, which the session's executive keeps and finds by handle.
fn is_application_attribute(a: &Attribute) -> bool {
    a.attribute_id() >= ids::USER_1
}

/// (not upstream) [`body_attributes`] of `body`'s application attributes alone
/// ([`is_application_attribute`]): what an operation that consumes and rebuilds `body`
/// (the cliff overflow's Boolean) carries on its objects, as upstream's live on them.
fn body_application_attributes(
    attributes: &AttributeStore,
    body: &Brep,
    serial: u64,
) -> Result<AttributeStore> {
    let mut store = body_attributes(attributes, body, serial)?;
    for key in store.attributed_objects() {
        for h in store.attributes(key) {
            if store.get(h).is_some_and(|a| !is_application_attribute(a)) {
                store.remove_attribute(key, h, true)?;
            }
        }
    }
    store.retain_listed();
    Ok(store)
}

/// (not upstream) Replaces, in the session store, the application attributes of the
/// objects issued `serial` by those `body`'s own store holds (pair of
/// [`body_application_attributes`], once the operation that carried them succeeded):
/// the old lists' application entries are dropped (their attributes left without those
/// users), then `body`'s are seeded and taken from it.
fn replace_application_attributes(
    attributes: &mut AttributeStore,
    body: &mut Brep,
    serial: u64,
) -> Result<()> {
    for key in attributes.attributed_objects() {
        if crate::topology::brep::p3::topology_entity(serial, key).is_none() {
            continue;
        }
        for h in attributes.attributes(key) {
            if attributes.get(h).is_some_and(is_application_attribute) {
                attributes.remove_attribute(key, h, true)?;
            }
        }
    }
    seed_body_attributes(attributes, body, serial)?;
    body.take_attributes();
    Ok(())
}

/// `~SmFilletExecutive`: `m_pAttributeE` leaves each of its users and is deleted.
/// (not upstream) Then the bodies get their attributes back from the session store.
impl Drop for FilletSession<'_> {
    fn drop(&mut self) {
        if let Some(FilletObject::Attribute(handle)) = self.executive.edge_attribute {
            // a handle the session allocated; nothing to report from a destructor
            let _ = self.objects.attributes.destroy(handle);
        }
        let objects = &mut self.objects;
        let (target_before, second_before) = core::mem::take(&mut objects.attributes_before);
        // (a hand-back that fails puts the body's store from before the session back,
        // rather than leaving it empty)
        let store = body_attributes(&objects.attributes, objects.target, objects.target_serial)
            .unwrap_or(target_before);
        objects.target.set_attributes(store);
        if let Some(second) = &mut objects.second_base {
            let store = body_attributes(&objects.attributes, second.body, second.serial)
                .unwrap_or(second_before);
            second.body.set_attributes(store);
        }
    }
}

impl<'target> FilletSession<'target> {
    /// Construct the native executive-owned objects, with its ordinary defaults.
    ///
    /// # Errors
    /// Context B-rep construction, attribute allocation or identity exhaustion.
    pub fn new(k: &mut Kernel, target: &'target mut Brep) -> Result<Self> {
        Self::with_second_base(k, target, None)
    }

    /// [`Self::new`] for a surface/surface fillet whose second base face lies in
    /// `second_base`, a B-rep other than `target` (upstream the two original surfaces
    /// may come from different B-reps; each rail merges into its base face's B-rep).
    /// Only [`Self::surface_surface_fillet`] reads the second base B-rep.
    ///
    /// # Errors
    /// Those of [`Self::new`].
    pub fn with_second_base(
        k: &mut Kernel,
        target: &'target mut Brep,
        second_base: Option<&'target mut Brep>,
    ) -> Result<Self> {
        let gn_scope = OperationScope::new(k);
        let target_serial = crate::topology::brep::p3::notify_serial(&mut k.context, target)?;
        let mut second_base = match second_base {
            Some(body) => Some(SecondBase {
                serial: crate::topology::brep::p3::notify_serial(&mut k.context, body)?,
                body,
                intersector: None,
            }),
            None => None,
        };
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
        // (not upstream: upstream the attributes stay on the bodies' objects.) The
        // session's store holds the bodies' own attributes under its identities while it
        // runs, after the executive's own; `Drop` hands them back.
        seed_body_attributes(&mut attributes, target, target_serial)?;
        if let Some(second) = &second_base {
            seed_body_attributes(&mut attributes, &*second.body, second.serial)?;
        }
        // Every seed succeeded: only now do the bodies' own stores move to the session.
        let attributes_before = (
            target.take_attributes(),
            second_base
                .as_mut()
                .map(|second| second.body.take_attributes())
                .unwrap_or_default(),
        );
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
                second_base,
                pseudo,
                intermediate: None,
                insertion_curves: insertion::curves::InsertionCurves::default(),
                notification_records: crate::topology::topology::NotifyRecords::new(),
                solvers: Vec::new(),
                corners: Vec::new(),
                generators: Vec::new(),
                laws: Vec::new(),
                attributes,
                attributes_before,
                merge_attributes: None,
                cleanup_kept: None,
                extended_surfaces: BTreeMap::new(),
                edge_caches: BTreeMap::new(),
                target_serial,
                self_intersection_handler: None,
                curve_blend_self_intersection_handler: None,
                swept_turns: false,
                set_back_vertices: Vec::new(),
                set_back_fills: Vec::new(),
                radius_corner_surfaces: Vec::new(),
                setback_refusal: None,
                blend_face_marks: Vec::new(),
                chamfer_corner: crate::blend::blend_corner::BlendChamferCorner::Mitre,
            },
            _gn_scope: gn_scope,
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
            None,
        )
    }

    /// [`FilletSession::new_constant_radius_solver`] with `SmConstantRadiusFS`'s
    /// `pdOptSecondRadius`: the offset radius of the second surface (`radius` is the
    /// first's), for the unequal setbacks of a chamfer.
    ///
    /// # Errors
    /// Those of [`FilletSession::new_constant_radius_solver`].
    #[allow(clippy::too_many_arguments)] // the native constructor's parameters
    pub fn new_constant_radius_solver_with_second(
        &mut self,
        k: &Kernel,
        edgeuse: EdgeuseId,
        radius: f64,
        second_radius: f64,
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
            Some(second_radius),
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
        self.refuse_rounds_too_small(k, edges, &[radius], &[xsect])?;
        let edges = edges
            .iter()
            .map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(*id)))
            .collect::<Vec<_>>();
        self.executive.set_do_global_merge(true);
        let prepared = self.executive.set_fillet_parameters(
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
                second_radii: None,
            },
        );
        self.typed_unblendable(k, prepared)
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
        self.prepare_per_edge(
            k,
            edges,
            radii,
            xsect_types,
            FilletSolverType::ConstRadius,
            continuity,
            thumbweight,
        )
    }

    /// [`FilletSession::prepare_per_edge_constant_radius`] with the fork's
    /// `pOptSecondRadii` (tangency-labs/smlib `f653ac9`): `second_radii` holds one entry
    /// per edge, the offset of the second edge use's surface (`radii` gives the first's).
    /// With a linear cross-section that is a chamfer with setbacks `radii[i]` and
    /// `second_radii[i]`. It runs the same `SetFilletParameters`, so the fork's vertex-fan
    /// split (`fixes.fillet_corner_per_vertex_fan`) applies to it.
    ///
    /// # Errors
    /// Those of [`FilletSession::prepare_per_edge_constant_radius`]; native invalid input
    /// when `second_radii` does not hold one entry per edge.
    #[allow(clippy::too_many_arguments)] // the native overload's parameters
    pub fn prepare_per_edge_constant_radius_with_second(
        &mut self,
        k: &mut Kernel,
        edges: &[EdgeId],
        radii: &[f64],
        second_radii: &[f64],
        xsect_types: &[FilletSurfaceGeneratorType],
        continuity: u32,
        thumbweight: f64,
    ) -> Result<()> {
        self.prepare_per_edge_with_second(
            k,
            edges,
            radii,
            Some(second_radii),
            xsect_types,
            FilletSolverType::ConstRadius,
            continuity,
            thumbweight,
        )
    }

    /// [`FilletSession::prepare_per_edge_constant_radius`] with constant-distance solvers
    /// (`SM_FS_CONST_DIST`): `distances` are the chords across each blend.
    ///
    /// # Errors
    /// Those of [`FilletSession::prepare_per_edge_constant_radius`].
    pub fn prepare_per_edge_constant_distance(
        &mut self,
        k: &mut Kernel,
        edges: &[EdgeId],
        distances: &[f64],
        xsect_types: &[FilletSurfaceGeneratorType],
        continuity: u32,
        thumbweight: f64,
    ) -> Result<()> {
        self.prepare_per_edge(
            k,
            edges,
            distances,
            xsect_types,
            FilletSolverType::ConstDist,
            continuity,
            thumbweight,
        )
    }

    /// The per-edge `SetFilletParameters` overload (`SmFilletExecutive.cpp:865`) with a
    /// solver type per edge (`radius_types`, one per edge or one for all): what the
    /// SolidCraft fork's `SmOffsetFacesExecutive` calls on a bare `SmFilletExecutive`
    /// (not upstream's API wrappers: no `SetDoGlobalMerge`), default tolerances and no
    /// laws.
    ///
    /// # Errors
    /// Native invalid input for an empty edge list or mismatched list sizes; real
    /// parameter/constructor/loader errors.
    #[allow(clippy::too_many_arguments)] // the native overload's parameters
    pub(crate) fn prepare_per_edge_solvers(
        &mut self,
        k: &mut Kernel,
        edges: &[EdgeId],
        sizes: &[f64],
        xsect_types: &[FilletSurfaceGeneratorType],
        radius_types: &[FilletSolverType],
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
        let edges = edges
            .iter()
            .map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(*id)))
            .collect::<Vec<_>>();
        let prepared = self.executive.set_fillet_parameters(
            k,
            &mut self.objects,
            &FilletParameters {
                brep: Some(FilletObject::Body(FilletBody::Target)),
                edges: &edges,
                xsect_types,
                radius_types,
                radius_values: sizes,
                var_rad_laws: None,
                approx_tol: -1.0,
                ang_tol_deg: -1.0,
                tan_tol_deg: -1.0,
                xsect_tol: -1.0,
                continuity,
                thumbweight,
                second_radii: None,
            },
        );
        self.typed_unblendable(k, prepared)
    }

    #[allow(clippy::too_many_arguments)] // the native overload's parameters
    fn prepare_per_edge(
        &mut self,
        k: &mut Kernel,
        edges: &[EdgeId],
        radii: &[f64],
        xsect_types: &[FilletSurfaceGeneratorType],
        radius_type: FilletSolverType,
        continuity: u32,
        thumbweight: f64,
    ) -> Result<()> {
        self.prepare_per_edge_with_second(
            k,
            edges,
            radii,
            None,
            xsect_types,
            radius_type,
            continuity,
            thumbweight,
        )
    }

    #[allow(clippy::too_many_arguments)] // the native overload's parameters
    fn prepare_per_edge_with_second(
        &mut self,
        k: &mut Kernel,
        edges: &[EdgeId],
        radii: &[f64],
        second_radii: Option<&[f64]>,
        xsect_types: &[FilletSurfaceGeneratorType],
        radius_type: FilletSolverType,
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
        if radius_type == FilletSolverType::ConstRadius && second_radii.is_none() {
            self.refuse_rounds_too_small(k, edges, radii, xsect_types)?;
        }
        let radius_types = vec![radius_type; edges.len()];
        let edges = edges
            .iter()
            .map(|id| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(*id)))
            .collect::<Vec<_>>();
        self.executive.set_do_global_merge(true);
        let prepared = self.executive.set_fillet_parameters(
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
                second_radii,
            },
        );
        self.typed_unblendable(k, prepared)
    }

    /// `SetDoGlobalMerge(TRUE)` then the same-parameters `SetFilletParameters`
    /// overload with `SM_FS_VARIABLE_RADIUS`, as `SmApiVariableRadiusFillet` does:
    /// one cross-section type for every edge, `radius` as the (unused) solver
    /// distance, `law` shared by every edge's solver and default tolerances. The
    /// thumbweight is `SetFilletParameters`' default: upstream's `1.0`, or with
    /// `fixes.fillet_gn_section_generator` the fork's `SM_GN_DEFAULT_THUMBWEIGHT`.
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
        let prepared = self.executive.set_fillet_parameters(
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
                // SetFilletParameters' default thumbweight: upstream's 1.0, the fork's
                // SM_GN_DEFAULT_THUMBWEIGHT with its G^n section (WR-685), which refuses 1.0
                thumbweight: if k.fixes.fillet_gn_section_generator {
                    crate::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT
                } else {
                    1.0
                },
                second_radii: None,
            },
        );
        self.typed_unblendable(k, prepared)
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
        // solvers loaded since the last sync (`SetFilletParameters`) read the handler
        // during the corners' traces
        self.sync_self_intersection_handler();
        // (not upstream, solidcore #2541) a lone round's solver takes the rims it may end
        // on before the corners trace it
        self.end_round_on_outline(k)?;
        let SessionObjects {
            target,
            solvers,
            corners,
            swept_turns,
            set_back_vertices,
            chamfer_corner,
            set_back_fills,
            ..
        } = &mut self.objects;
        let swept_turns = *swept_turns;
        let chamfer_corner = *chamfer_corner;
        let set_back_vertices = &*set_back_vertices;
        let set_back_fills = &*set_back_fills;
        self.executive
            .create_fillet_corners_recording_vertex_errors(
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
                // the faces an earlier pass of this blend made (the fork's 9822462)
                let earlier_pass_faces: Vec<FaceId> = executive
                    .earlier_pass_faces
                    .iter()
                    .filter_map(|face| match face {
                        FilletObject::Topology(FilletBody::Target, EntityRef::Face(face)) => {
                            Some(*face)
                        }
                        _ => None,
                    })
                    .collect();
                let made = FilletCorner::create_with_solver_lookup(
                    k,
                    target,
                    vertex,
                    &edgeuses,
                    &earlier_pass_faces,
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
                let Some(mut corner) = made else {
                    // The vertex's point feeds the opt-in vertex error note, as the
                    // fork's corrected overload reads it from the vertex itself.
                    let location = crate::topology::vertex::point(target, vertex).ok();
                    return Ok(CornerCreation::Unsupported { location });
                };
                // Not the fork's (WR-716, #2345): chamfers meeting at a vertex where every
                // edge is chamfered are the blend corner's chamfer event, closed by their
                // planes, of the session's corner type; refused where that type does not
                // exist there; a corner the event declines stays the pinned 3x3 one
                if k.fixes.fillet_chamfer_corner
                    && corner.chamfer
                    && corner.kind == FilletCornerKind::ConvexNByN
                {
                    use crate::blend::blend_corner::chamfer::{classify_chamfer, ChamferClass};
                    let lookup = SweptSessionSolvers {
                        solvers: solvers.as_slice(),
                        executive: Some(executive),
                    };
                    let all_eus = corner.all_eus.clone().unwrap_or_default();
                    match classify_chamfer(
                        k,
                        target,
                        vertex,
                        &all_eus,
                        chamfer_corner,
                        corner.approx_tol_3d,
                        &lookup,
                    )? {
                        ChamferClass::Event(event) => {
                            corner.blend = Some(crate::blend::blend_corner::BlendCorner {
                                event: crate::blend::blend_corner::BlendCornerEvent::Chamfer,
                                turn: crate::blend::blend_corner::BlendTurn::Mitre,
                                end: crate::blend::blend_corner::BlendEnd::OnEndFace,
                            });
                            corner.chamfer_corner = Some(*event);
                        }
                        ChamferClass::Refused(refusal) => {
                            let location = crate::topology::vertex::point(target, vertex).ok();
                            return Ok(CornerCreation::Refused {
                                location,
                                error: refusal.error,
                                comment: refusal.comment,
                                fit_edge: refusal.fit_edge.map(|e| {
                                    FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e))
                                }),
                                fit: refusal.fit,
                            });
                        }
                        ChamferClass::Declined(_) => {}
                    }
                }
                // The fork's (WR-697, 79d4763): three equal rounds of one convexity, every
                // edge there blended, are the blend corner's Ball
                // (SmBlendCorner::ClassifyBall, in SmFilletCorner::Create's three-solver
                // case before the 3x3 corner); a failed classification builds no corner.
                // Since smlib#125 (278bf77) four or more rounds that one ball touches are
                // the Ball too (ClassifyBall in the NxN case, before ClassifySetBack, where
                // every edge use is a solver's). A chamfer corner's rounds are not
                // circular, so ClassifyBall finds no Ball there before any test that could
                // fail.
                if k.fixes.fillet_blend_corner
                    && matches!(
                        corner.kind,
                        FilletCornerKind::ConvexNByN | FilletCornerKind::ConcaveNByN
                    )
                    && corner.all_eus.as_ref().map_or(0, Vec::len) >= 3
                    && corner.solver_eus.as_ref().map_or(0, Vec::len)
                        == corner.all_eus.as_ref().map_or(0, Vec::len)
                    && !corner.chamfer
                {
                    let lookup = SweptSessionSolvers {
                        solvers: solvers.as_slice(),
                        executive: Some(executive),
                    };
                    let all_eus = corner.all_eus.clone().unwrap_or_default();
                    // a vertex asked to be set back is no Ball, by either door: marked
                    // (AddSetBackVertex, IsSetBackVertex) or asked a fill (SetSetBackFill,
                    // which only the Setback builds; solidcore#2365)
                    let set_back = set_back_vertices.contains(&vertex)
                        || crate::blend::blend_corner::setback::set_back_fill_of(
                            set_back_fills,
                            vertex,
                        )
                        .is_some();
                    let event = crate::blend::blend_corner::ball::classify_ball(
                        k,
                        target,
                        vertex,
                        &edgeuses,
                        &all_eus,
                        set_back,
                        &lookup,
                    );
                    match event {
                        Err(_) => {
                            let location = crate::topology::vertex::point(target, vertex).ok();
                            return Ok(CornerCreation::Unsupported { location });
                        }
                        Ok(crate::blend::blend_corner::BlendCornerEvent::Ball) => {
                            let mut ball = FilletCorner::new(
                                FilletCornerKind::NByTwo,
                                corner.vertex,
                                corner.solver_eus.clone(),
                                corner.all_eus.clone(),
                                corner.approx_tol_3d,
                                corner.tangency_tol_radians,
                            );
                            ball.blend = Some(crate::blend::blend_corner::BlendCorner {
                                event: crate::blend::blend_corner::BlendCornerEvent::Ball,
                                turn: crate::blend::blend_corner::BlendTurn::Mitre,
                                end: crate::blend::blend_corner::BlendEnd::OnEndFace,
                            });
                            corner = ball;
                        }
                        Ok(_) => {}
                    }
                }
                // The fork's (smlib#104 61547af): a corner where every edge is blended,
                // three or more, that the blend corner sets back and fills
                // (SmBlendCorner::ClassifySetBack, in SmFilletCorner::Create's NxN case,
                // after the Ball); a failed classification builds no corner. The others as
                // before.
                if k.fixes.fillet_blend_corner
                    && corner.blend.is_none()
                    && matches!(
                        corner.kind,
                        FilletCornerKind::ConvexNByN | FilletCornerKind::ConcaveNByN
                    )
                    && !corner.chamfer
                {
                    let lookup = SweptSessionSolvers {
                        solvers: solvers.as_slice(),
                        executive: Some(executive),
                    };
                    let all_eus = corner.all_eus.clone().unwrap_or_default();
                    let classified = crate::blend::blend_corner::setback::classify_setback(
                        k,
                        target,
                        vertex,
                        &edgeuses,
                        &all_eus,
                        &lookup,
                        crate::math::rad_to_deg(corner.tangency_tol_radians),
                        set_back_fills,
                    );
                    match classified {
                        Err(_) => {
                            let location = crate::topology::vertex::point(target, vertex).ok();
                            return Ok(CornerCreation::Unsupported { location });
                        }
                        Ok((crate::blend::blend_corner::BlendCornerEvent::Setback, order)) => {
                            let mut setback = FilletCorner::new(
                                FilletCornerKind::NByTwo,
                                corner.vertex,
                                corner.solver_eus.clone(),
                                corner.all_eus.clone(),
                                corner.approx_tol_3d,
                                corner.tangency_tol_radians,
                            );
                            setback.blend = Some(crate::blend::blend_corner::BlendCorner {
                                event: crate::blend::blend_corner::BlendCornerEvent::Setback,
                                turn: crate::blend::blend_corner::BlendTurn::Mitre,
                                end: crate::blend::blend_corner::BlendEnd::OnEndFace,
                            });
                            setback.setback = Some(
                                crate::blend::blend_corner::setback::SetbackCorner::with_order(
                                    order,
                                ),
                            );
                            corner = setback;
                        }
                        Ok(_) => {}
                    }
                }
                // solidcore-native (WR-697 addendum; the fork refuses this only in its capi): a
                // corner asked to be set back (SetSetBackFill) that builds no setback at all
                // (neither the Setback nor an N x N corner) would drop it silently, so it is
                // refused, decided from the request and the classification alone (the second
                // door, SetSetBackDist on the made corner, is refused by `do_filleting` on
                // the same `builds_setback`).
                if k.fixes.fillet_blend_corner
                    && crate::blend::blend_corner::setback::set_back_fill_of(
                        set_back_fills,
                        vertex,
                    )
                    .is_some()
                    && !crate::blend::blend_corner::setback::builds_setback(&corner)
                {
                    let location = crate::topology::vertex::point(target, vertex).ok();
                    return Ok(CornerCreation::Refused {
                        location,
                        error: crate::blend::fillet_geom::p1::FilletErrorType::CornerUnsupported,
                        comment: crate::blend::blend_corner::setback::SETBACK_NOT_BUILT.to_owned(),
                        fit_edge: None,
                        fit: None,
                    });
                }
                // The fork's (WR-697, e647dac): with GetSweptTurns() a blend corner's
                // Turn is Swept, or refused with its reason (SmBlendCorner::Classify)
                if let Some(blend) = corner.blend.as_mut().filter(|blend| {
                    k.fixes.fillet_blend_corner
                        && swept_turns
                        && blend.event == crate::blend::blend_corner::BlendCornerEvent::Turn
                }) {
                    blend.turn = crate::blend::blend_corner::BlendTurn::Swept;
                    let lookup = SweptSessionSolvers {
                        solvers: solvers.as_slice(),
                        executive: Some(executive),
                    };
                    if let Some(refusal) = crate::blend::blend_corner::swept::classify_swept(
                        k,
                        target,
                        vertex,
                        &edgeuses,
                        corner.all_eus.as_deref().unwrap_or_default(),
                        crate::math::rad_to_deg(corner.tangency_tol_radians),
                        &lookup,
                    )? {
                        let location = crate::topology::vertex::point(target, vertex).ok();
                        return Ok(CornerCreation::Refused {
                            location,
                            error: refusal.error,
                            comment: refusal.comment,
                            fit_edge: refusal.fit_edge.map(|e| {
                                FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e))
                            }),
                            fit: None,
                        });
                    }
                }
                let id = FilletCornerId(
                    u32::try_from(corners.len()).map_err(|_| invalid("too many corners"))?,
                );
                corners.push(corner);
                Ok(CornerCreation::Created(FilletObject::Corner(id)))
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
        self.objects
            .calc_solver_geometry(k, id, &mut self.executive)
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

    /// `GetPreviewFilletSurfaces`: `CreateFilletCorners` (its default null error info),
    /// then [`FilletExecutive::preview_fillet_geometry`], then the fillet surface of each
    /// retained solver's fillet geometries in order (`GetFilletGeoms`), `None` for a
    /// geometry without one (upstream adds the null pointer). The surfaces are exact
    /// copies: the session's geometries keep the originals, which upstream hands out
    /// (and never frees).
    ///
    /// # Errors
    /// Those of [`Self::create_corners`] and the preview stages, retaining their
    /// mutations.
    pub fn preview_fillet_surfaces(&mut self, k: &mut Kernel) -> Result<Vec<Option<Surface>>> {
        // NER(m_pTargetBrep) comes first upstream; a session always has its target
        self.create_corners(k, None)?;
        self.executive
            .preview_fillet_geometry(k, &mut self.objects)?;
        let mut surfaces = Vec::new();
        let mut index = 0;
        while let Some(solver) = self.executive.solvers.get(index).copied() {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("preview solver is not an actual solver"));
            };
            for geom in self.objects.solver(id)?.base().fillet_geoms() {
                let surface = self.objects.fillet_surface(geom)?;
                surfaces.push(surface.map(Surface::clone_for_evaluation).transpose()?);
            }
            index += 1;
        }
        Ok(surfaces)
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
        // the executive holds one handler
        self.objects.curve_blend_self_intersection_handler = None;
        self.executive
            .set_self_intersection_handler(handler.map(|_| FilletObject::SelfIntersectionHandler));
        self.sync_self_intersection_handler();
    }

    /// `SetSelfIntersectionHandler` with an `SmMakeCurveBlendSIH`: retain `handler`
    /// as the executive's (only) handler, replacing a surface-blend one, so every
    /// `FlushCurve` of a loaded solver runs its `SmoothOutIntersection`.
    pub fn set_curve_blend_self_intersection_handler(
        &mut self,
        handler: Option<MakeCurveBlendSih>,
    ) {
        self.objects.self_intersection_handler = None;
        self.objects.curve_blend_self_intersection_handler = handler;
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
        let curve_blend = self.objects.curve_blend_self_intersection_handler;
        let algorithm = algorithm.or_else(|| curve_blend.map(|h| h.algorithm_type()));
        for solver in self.objects.solvers.iter_mut().flatten() {
            let base = solver.base_mut();
            if base.executive.is_some() {
                base.executive_self_intersection_handler = algorithm;
                base.executive_curve_blend_sih = curve_blend;
            }
        }
    }

    /// `GetFilletErrorInfo`: what the executive recorded of its last failure
    /// (`SmFilletErrorInfo`; the code is the `SmFilletErrorType` value, `FilletErrorType::NoRoom` = 8 for the
    /// fork's `SM_FILERR_NO_ROOM`).
    #[must_use]
    pub const fn error_info(&self) -> &ErrorInfo<FilletObject> {
        &self.executive.error
    }

    /// The fork's `SetSweptTurns` (smlib#79 step 4b; WR-697): with
    /// `Kernel::fixes.fillet_blend_corner`, a blend corner's Turn (two rounds of one
    /// convexity that turn at a sharp side edge) is Swept, its corner face the rounds' end
    /// section revolved about the side edge (an exact torus), instead of Mitred; a Swept
    /// turn the blend corner does not build is refused with its reason. Set it before
    /// [`Self::do_fillet`] (the corners are made there). Without the flag it changes
    /// nothing.
    pub fn set_swept_turns(&mut self, swept: bool) {
        self.objects.swept_turns = swept;
    }

    /// How chamfers meeting at a vertex where every edge is chamfered close the corner,
    /// with `Kernel::fixes.fillet_chamfer_corner` (WR-716): the Mitre (the default; the
    /// chamfer planes run on to meet each other) or the Facet (one plane through the
    /// rails' crossings). It overrides the corner's bevel flag
    /// ([`Self::set_corner_bevel`]) at such a corner. Set it before [`Self::do_fillet`]
    /// (the corners are made there). Without the flag it changes nothing.
    pub fn set_chamfer_corner(&mut self, corner: crate::blend::blend_corner::BlendChamferCorner) {
        self.objects.chamfer_corner = corner;
    }

    /// The fork's `AddSetBackVertex` (smlib#79 step 4c; WR-697): the caller sets
    /// `vertex` back (its corner's setback distance, set after the corners are made), so
    /// with `Kernel::fixes.fillet_blend_corner` no Ball is built there
    /// (`IsSetBackVertex`, read by `SmBlendCorner::ClassifyBall`). Add it before
    /// [`Self::do_fillet`]. Without the flag it changes nothing.
    pub fn add_set_back_vertex(&mut self, vertex: VertexId) {
        self.objects.set_back_vertices.push(vertex);
    }

    /// The fork's `SetSetBackFill` (smlib#104 61547af): the corner at `vertex` set back
    /// `setback` along each of its edges, with an ease `ease` before it (0: none): asked
    /// before [`Self::do_fillet`], which then makes that corner the blend corner's Setback
    /// (the vertex is no Ball, as one marked with [`Self::add_set_back_vertex`] is not),
    /// filled by the setback split, where it can (with `Kernel::fixes.fillet_blend_corner`).
    /// Left `open`, a G1 corner is the Setback's hole (its spring curves, no fill, ef57570);
    /// only a G2 corner (eased, or curvature-continuous sections) stays the fill's patches
    /// the caller removes. A corner asked for here that is not the Setback is refused
    /// (`CornerUnsupported`), except an N x N corner, which ignores the request as before
    /// (it builds a setback given with [`Self::set_corner_setback`] on its own setback
    /// path). Without the flag it changes nothing.
    pub fn set_set_back_fill(&mut self, vertex: VertexId, setback: f64, ease: f64, open: bool) {
        self.objects
            .set_back_fills
            .push(crate::blend::blend_corner::setback::SetBackFill {
                vertex,
                setback,
                ease,
                open,
            });
    }

    /// The fork's `GetFitSetBack` (smlib#104 f1d81f4): with a setback corner refused as too
    /// short for a convex fill (`FilletErrorType::NoRoom`), the least setback that builds
    /// one: `> 0`, `0` when none does, `< 0` when not that cause. Only with
    /// `fixes.fillet_blend_corner`.
    #[must_use]
    pub fn fit_setback(&self) -> f64 {
        self.executive.fit_setback
    }

    /// `GetFitRadius`, `GetFitEdge`, `GetFitKind` (the fork's, tangency-labs/smlib#79
    /// a3d6d81): with a refusal as too large ([`ErrorInfo::no_room_reason`]), the largest
    /// size that builds the refused blend and its edge, computed from the refusal's cause;
    /// `None` when its cause has no size (a radius law) or no refusal noted one. Only with
    /// `fixes.fillet_gn_section_generator`.
    #[must_use]
    pub fn fit_size(&self) -> Option<crate::blend::fillet_executive::FitSize<EdgeId>> {
        let fit = self.executive.fit_size()?;
        let edge = match fit.edge {
            Some(FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e))) => Some(e),
            _ => None,
        };
        Some(crate::blend::fillet_executive::FitSize {
            size: fit.size,
            edge,
            kind: fit.kind,
        })
    }

    /// `SetDoGlobalMerge`.
    pub fn set_do_global_merge(&mut self, do_global_merge: bool) {
        self.executive.set_do_global_merge(do_global_merge);
    }

    /// `SetDoPiecewiseMerge`: `DoFilleting` then merges the intermediate body into the
    /// target one face at a time (`DoPiecewiseMerge`). `SetFilletParameters` resets
    /// it, so set it after preparing the edges.
    pub fn set_do_piecewise_merge(&mut self, do_piecewise_merge: bool) {
        self.executive.options.piecewise_merge = do_piecewise_merge;
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

    /// `SmFilletCorner::SetSetBackDist` on one live corner. With
    /// `Kernel::fixes.fillet_blend_corner`, a setback (`distance > 0`) set on a corner that
    /// builds none (neither the blend corner's Setback nor an N x N corner,
    /// `blend_corner::setback::builds_setback`) is refused by [`Self::do_filleting`]
    /// (`CornerUnsupported`) rather than built without it.
    ///
    /// # Errors
    /// An unresolved corner identity.
    pub fn set_corner_setback(&mut self, id: FilletCornerId, distance: f64) -> Result<()> {
        self.objects
            .corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live in this session"))?
            .set_back_dist = distance;
        Ok(())
    }

    /// `SmFilletCorner::SetEaseDist` on one live corner (the fork's, tangency-labs/smlib
    /// `e74bbd3`, #1376): over `distance` along each edge before the corner's setback, each
    /// round's cross-section eases from its own into the curvature-continuous one, and the
    /// corner's rail-rail curves meet the rails curvature-continuously. `0.0` (the default)
    /// is none, the pinned behaviour; it acts only with a setback
    /// ([`Self::set_corner_setback`]).
    ///
    /// # Errors
    /// An unresolved corner identity.
    pub fn set_corner_ease(&mut self, id: FilletCornerId, distance: f64) -> Result<()> {
        self.objects
            .corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live in this session"))?
            .ease_dist = distance;
        Ok(())
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

/// The operation of the fork's `SmFilletExecutive::NoteOpenEnd` (WR-631).
const OP_NOTE_OPEN_END: &str = "blend::fillet_executive::session::note_open_end";

impl CornerEdgeAccess<FilletObject> for SessionObjects<'_> {
    fn edges_adjust_tangent(
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
        // The running corner is taken out of its slot while it adjusts; its own vertex
        // calculation puts it back for the duration (TangentSplitOps).
        let slot = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let placeholder = FilletCorner::new(FilletCornerKind::Open, None, None, None, 0.0, 0.0);
        let mut corner = std::mem::replace(slot, placeholder);
        let mut ops = SessionTangentSplit {
            executive,
            solvers: solvers.as_mut_slice(),
            corners,
            extended_surfaces,
            id,
        };
        let mut env = TangentSplitEnv {
            k,
            target,
            pseudo,
            corner: Some(id),
            ops: &mut ops,
        };
        let result = corner.adjust_tangent_corner_with(
            &mut env,
            FilletCorner::adjust_tangent_corner_nx2,
            FilletCorner::adjust_tangent_corner_nx1_closed,
        );
        if let Some(slot) = ops.corners.get_mut(id.0 as usize) {
            *slot = corner;
        }
        result
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
            setback_refusal,
            extended_surfaces,
            set_back_fills,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        // The fork's (WR-697, 32fd80a): a blend corner's Mitre turn makes its mitre
        // exact where the rounds' class allows (SmBlendCorner::CalcCornerEdgeGeom), before
        // the edges' own kinds compute the rest
        if corner
            .blend
            .is_some_and(|blend| blend.is_mitre_turn(corner.all_eus.as_deref()))
        {
            struct Live<'s>(&'s mut [Option<SolverObject>]);
            impl crate::blend::blend_corner::MitreSolvers for Live<'_> {
                fn ops(&mut self, id: FilletSolverId) -> Result<&mut dyn FilletSolverOps> {
                    self.0
                        .get_mut(id.0 as usize)
                        .and_then(Option::as_mut)
                        .map(live_solver_ops)
                        .ok_or_else(|| invalid("mitre solver is not live"))
                }
            }
            let mut live = Live(solvers.as_mut_slice());
            for e in corner.edges.clone() {
                let edge = pseudo.fillet_edge(e)?;
                if edge.fillet_edge_type()
                    != crate::blend::fillet_geom::p1::FilletEdgeType::FilletXFillet
                    || pseudo.brep().edge(e)?.curve.is_some()
                {
                    continue;
                }
                crate::blend::blend_corner::exact_mitre_curve(
                    k, target, pseudo, corner, e, &mut live,
                )?;
            }
        }
        // The fork's (WR-697, 79d4763): a Ball's patch and its loop
        // (SmBlendCorner::BuildBallSurface, CalcCornerEdgeGeom), before the edges' own
        // kinds compute their curves
        if corner
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_ball)
        {
            let lookup = SweptSessionSolvers {
                solvers: solvers.as_slice(),
                executive: None,
            };
            crate::blend::blend_corner::ball::ball_corner_edge_geom(
                k, target, pseudo, corner, &lookup,
            )?;
        }
        // WR-716: the chamfer event's edges as exact lines and its facet's plane, before
        // the edges' own kinds would march them
        if corner
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_chamfer)
        {
            crate::blend::blend_corner::chamfer::chamfer_corner_edge_geom(k, pseudo, corner)?;
        }
        // The fork's (61547af, smlib#104): a Setback's fill, its base points and its spring
        // curves (SmBlendCorner::BuildSetBackFill, CalcCornerEdgeGeom), before the edges' own
        // kinds compute their curves
        if corner
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_setback)
        {
            let refused = {
                let mut live = SetbackSessionSolvers {
                    solvers: solvers.as_mut_slice(),
                    executive: &*executive,
                    corner: id,
                };
                crate::blend::blend_corner::setback::build_setback_fill(
                    k,
                    target,
                    pseudo,
                    corner,
                    &mut live,
                    set_back_fills,
                )?
            };
            if let Err(refusal) = refused {
                note_setback_refusal(executive, target, corner.filleted_vertex(), &refusal);
                *setback_refusal = refusal.refusal;
                return Err(setback_refused("SmBlendCorner::BuildSetBackFill", &refusal));
            }
        }
        // The fork's (WR-697, 6fd0411, 6e79de5): a Mixed corner's edges and its two faces'
        // surfaces (SmBlendCorner::CalcMixedEdgeGeom), before the base's
        // CalcCornerEdgeGeom; its edges are all precomputed
        if corner
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_mixed)
        {
            let lookup = SweptSessionSolvers {
                solvers: solvers.as_slice(),
                executive: None,
            };
            crate::blend::blend_corner::mixed::calc_mixed_edge_geom(
                k, target, pseudo, corner, &lookup,
            )?;
        }
        // The fork's (WR-697, e647dac): a Swept turn's torus and its arc A-B
        // (SmBlendCorner::BuildSweptSurface, CalcCornerEdgeGeom), before the edges' own
        // kinds compute the rest
        if corner
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_swept)
        {
            let lookup = SweptSessionSolvers {
                solvers: solvers.as_slice(),
                executive: None,
            };
            crate::blend::blend_corner::swept::swept_corner_edge_geom(
                k, target, pseudo, corner, &lookup,
            )?;
        }
        // WR-303: the Nx1 corner's end rib hands the round to the global merge; in a
        // cliff overflow (WR-631) the end is left open, noted for the subtraction.
        let cliff_overflow = executive.cliff_overflow;
        // the rails' rollover follows the corners' edges, not in a cliff overflow (#2406)
        let rolls_over = executive.options.classification && !cliff_overflow;
        let mut open_ends = Vec::new();
        let mut note_open_end = |eu, vertex, end_face, section| -> Result<()> {
            // GetFilletSolverOfEdgeuse(pFilEdgeuse), then NoteOpenEnd
            let solver = executive
                .fillet_solver_of_edgeuse(FilletObject::Topology(
                    FilletBody::Target,
                    EntityRef::Edgeuse(eu),
                ))
                .ok_or_else(|| SolidError::null_pointer(OP_NOTE_OPEN_END))?;
            open_ends.push(OpenEnd {
                solver,
                vertex: FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(vertex)),
                end_face: FilletObject::Topology(FilletBody::Target, EntityRef::Face(end_face)),
                section,
            });
            Ok(())
        };
        if k.fixes.fillet_blend_corner
            && corner
                .blend
                .is_some_and(crate::blend::blend_corner::BlendCorner::is_end)
        {
            // The fork's SmBlendCorner::CalcEndEdgeGeom (53c47b8, 41a39ec): an End's edges,
            // its rail past its side edge refused naming the radius that reaches it
            let lookup = SweptSessionSolvers {
                solvers: solvers.as_slice(),
                executive: Some(&*executive),
            };
            // Not the fork's (#2406): an End at a smooth vertex whose rail rolls over its
            // side face's tangent edge before the End's plane builds its edges once the
            // rails' rollover has traced the rail on the face beyond (the adjusted
            // corner's pass)
            if crate::blend::blend_corner::end::end_awaits_rollover(
                k, target, pseudo, corner, &lookup, rolls_over,
            )? {
                return Ok(());
            }
            let refusal = crate::blend::blend_corner::end::calc_end_edge_geom(
                k,
                target,
                pseudo,
                corner,
                &lookup,
                cliff_overflow,
                &mut note_open_end,
            )?;
            executive.open_ends.append(&mut open_ends);
            let refusal = match refusal {
                crate::blend::blend_corner::end::EndEdges::Built => None,
                crate::blend::blend_corner::end::EndEdges::GlobalMerge => {
                    executive.options.global_merge = true;
                    None
                }
                crate::blend::blend_corner::end::EndEdges::Refused(refusal) => Some(refusal),
            };
            if let Some(refusal) = refusal {
                executive.error.note_error(
                    crate::blend::fillet_geom::p1::FilletErrorType::NoRoom,
                    Some(&refusal.comment),
                );
                executive.note_fit_radius(
                    refusal.fit,
                    refusal
                        .edge
                        .map(|e| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e))),
                    crate::blend::fillet_executive::p3::FitKind::Radius,
                );
                return Err(SolidError::sm_err("SmBlendCorner::CalcEndEdgeGeom"));
            }
        } else {
            // WR-674: a round's end where its edge runs on tangentially is left open too.
            let tangent_ends = k.fixes.fillet_round_ends_at_tangent_continuation;
            let dropped = corner.drop_end_rib(
                k,
                target,
                pseudo,
                cliff_overflow,
                tangent_ends,
                &mut note_open_end,
            )?;
            executive.open_ends.append(&mut open_ends);
            if dropped == EndRibDrop::GlobalMerge {
                executive.options.global_merge = true;
            }
        }
        let mut context = SessionCornerEdgeContext {
            k,
            target,
            pseudo,
            lookup: SessionEdgeLookup {
                solvers,
                extended_surfaces,
            },
            corner: id,
            do_global_merge: executive.options.global_merge,
            error: &mut executive.error,
        };
        corner.calc_corner_edge_geom_with(
            &mut context,
            |context, corner, edge| {
                let corners = CornerLookup {
                    table: None,
                    this: Some((Some(context.corner), corner)),
                    exec: None,
                };
                // SmFilletCorner::RecordFilletError -> m_pExecutive->NoteFilletError:
                // the corner code notes into its own identities, replayed into the
                // executive's record.
                let mut note = ErrorInfo::default();
                let result = context
                    .pseudo
                    .calc_corner_edge_geom_with_context_and_error_info(
                        context.k,
                        edge,
                        corners,
                        &mut context.lookup,
                        context.target,
                        Some(&mut note),
                    );
                replay_corner_error(context.error, &note);
                result
            },
            |context| context.do_global_merge,
            |context| &mut *context.pseudo,
            |context, edge| context.pseudo.delete_wire_fillet_edge(context.k, edge),
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

    fn creation_ball_fits(&mut self, k: &mut Kernel, solver: FilletObject) -> Result<bool> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let index = id.0 as usize;
        let current = self
            .solvers
            .get_mut(index)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        crate::blend::fillet_solver::ball_fit::ball_fits_its_faces(
            live_solver_ops(current),
            k,
            self.target,
        )
    }

    fn creation_extend_trace_to_its_rails(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
    ) -> Result<()> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let current = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        trace_reach::extend_trace_to_its_rails(k, self.target, current)
    }

    fn creation_ball_is_regular(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
    ) -> Result<(ball_regular::BallStation, bool)> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let current = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let const_radius = current.solver_type()? == FilletSolverType::ConstRadius;
        let station = ball_regular::ball_is_regular(k, self.target, current)?;
        Ok((station, const_radius))
    }

    fn creation_end_fit_past_side_edges(
        &mut self,
        k: &Kernel,
        corner: FilletObject,
        executive: &FilletExecutive<FilletObject>,
    ) -> Result<Option<(f64, FilletObject)>> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        let SessionObjects {
            target,
            solvers,
            corners,
            ..
        } = self;
        let corner = corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        if corner.blend.is_none() {
            return Ok(None);
        }
        let lookup = SweptSessionSolvers {
            solvers: solvers.as_slice(),
            executive: Some(executive),
        };
        let const_radius = || -> Result<bool> {
            let eu = corner
                .solver_eus
                .as_deref()
                .and_then(|eus| eus.first().copied())
                .ok_or_else(|| invalid("corner without blended edge use"))?;
            let kind = lookup.live(eu)?.solver_type()?;
            Ok(matches!(
                kind,
                FilletSolverType::ConstRadius | FilletSolverType::ConstRadiusAssisted
            ))
        };
        Ok(crate::blend::blend_corner::end::fit_of_end_past_side_edges(
            k,
            target,
            corner,
            &lookup,
            const_radius,
        )?
        .map(|(fit, edge)| {
            (
                fit,
                FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)),
            )
        }))
    }

    fn creation_solver_fit(
        &self,
        solver: FilletObject,
    ) -> Result<Option<crate::blend::fillet_executive::p3::SolverFit<FilletObject>>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let current = self
            .solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let base = current.base();
        let Some(edgeuse) = base.edgeuse(0)? else {
            return Ok(None);
        };
        let Some(edge) = crate::topology::edgeuse::p1::edge(self.target, edgeuse)? else {
            return Ok(None);
        };
        // FitMargin: a thousand of the blended edge's tolerances
        let tolerance = crate::topology::edge::tolerance(self.target, edge)?;
        let fit_margin = 1000.0 * tolerance;
        let offsets = match (base.surface(0)?, base.surface(1)?) {
            (Some(a), Some(b)) => Some([a.surface.offset_distance(), b.surface.offset_distance()]),
            _ => None,
        };
        Ok(Some(crate::blend::fillet_executive::p3::SolverFit {
            edge: FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)),
            fit_margin,
            tolerance,
            offsets,
        }))
    }

    fn creation_band_cuts_the_part(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
    ) -> Result<Option<crate::blend::fillet_executive::p3::SolverCut<FilletObject>>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let current = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        // sm_BlendAddsMaterial: IsConcaveEdge of the solver's edge
        let edge = match current.base().edgeuse(0)? {
            Some(edgeuse) => crate::topology::edgeuse::p1::edge(self.target, edgeuse)?,
            None => None,
        };
        let edge = edge.ok_or_else(|| SolidError::null_pointer(OP))?;
        if crate::blend::fillet_solver::p5::is_concave_edge(k, self.target, edge)? {
            return Ok(None);
        }
        let Some(cut) = band_cuts::band_cuts_the_part(k, self.target, current)? else {
            return Ok(None);
        };
        let chamfer = current.base().fsg.as_ref().is_some_and(|fsg| {
            fsg.fillet_surface_generator_type() == FilletSurfaceGeneratorType::Linear
        });
        Ok(Some(crate::blend::fillet_executive::p3::SolverCut {
            radius: cut.radius,
            setback: cut.setback,
            edge: Some(FilletObject::Topology(
                FilletBody::Target,
                EntityRef::Edge(edge),
            )),
            chamfer,
        }))
    }

    fn creation_band_breaks_through(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
    ) -> Result<Option<crate::blend::fillet_executive::p3::SolverCut<FilletObject>>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let current = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        // a convex blend only (sm_BlendAddsMaterial: IsConcaveEdge of the solver's edge)
        let edge = match current.base().edgeuse(0)? {
            Some(edgeuse) => crate::topology::edgeuse::p1::edge(self.target, edgeuse)?,
            None => None,
        };
        let edge = edge.ok_or_else(|| SolidError::null_pointer(OP))?;
        if crate::blend::fillet_solver::p5::is_concave_edge(k, self.target, edge)? {
            return Ok(None);
        }
        let Some(cut) = band_cuts::band_breaks_through_the_part(k, self.target, current)? else {
            return Ok(None);
        };
        let chamfer = current.base().fsg.as_ref().is_some_and(|fsg| {
            fsg.fillet_surface_generator_type() == FilletSurfaceGeneratorType::Linear
        });
        Ok(Some(crate::blend::fillet_executive::p3::SolverCut {
            radius: cut.radius,
            setback: cut.setback,
            edge: Some(FilletObject::Topology(
                FilletBody::Target,
                EntityRef::Edge(edge),
            )),
            chamfer,
        }))
    }

    fn creation_largest_size_on_its_faces(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        only: Option<FilletObject>,
    ) -> Result<Option<(f64, crate::blend::fillet_executive::FitKind)>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let only = match only {
            None => None,
            Some(FilletObject::Topology(FilletBody::Target, EntityRef::Face(face))) => Some(face),
            Some(_) => return Err(invalid("the fit's face is not a target face")),
        };
        let current = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        fit_size::largest_size_on_its_faces(k, self.target, current, only)
    }

    fn creation_polygons_taken_whole(
        &mut self,
        k: &mut Kernel,
        solvers: &[FilletObject],
    ) -> Result<Vec<(f64, FilletObject, f64)>> {
        // the faces the blends roll on (the fork's map of face pointers, here in face order),
        // each with its blends in the executive's order
        let mut on: BTreeMap<FaceId, Vec<(usize, usize)>> = BTreeMap::new();
        for &solver in solvers {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("object is not a solver"));
            };
            let index = id.0 as usize;
            let current = self.solver(id)?;
            let (rolls, law) = ball_regular::rolls_a_ball(current);
            if !rolls || law {
                continue;
            }
            for rail in 0..2 {
                if let Some(edgeuse) = current.base().edgeuse(rail)? {
                    if let Some(face) = crate::topology::edgeuse::p1::face(self.target, edgeuse)? {
                        on.entry(face).or_default().push((index, rail));
                    }
                }
            }
        }
        let mut out = Vec::new();
        for (face, blends) in on {
            // each blend's solver taken out for the sizing, put back after
            let mut taken: Vec<(usize, usize, SolverObject)> = Vec::new();
            for &(index, rail) in &blends {
                if let Some(object) = self.solvers.get_mut(index).and_then(Option::take) {
                    taken.push((index, rail, object));
                }
            }
            // a solver on the face twice (both its faces) cannot be held twice: not sized
            let result = if taken.len() == blends.len() {
                let mut views: Vec<fit_size::OnFace<'_>> = taken
                    .iter_mut()
                    .map(|(_, rail, object)| fit_size::OnFace {
                        solver: object,
                        rail: *rail,
                    })
                    .collect();
                fit_size::polygon_fit(k, self.target, face, &mut views)
            } else {
                Ok(None)
            };
            for (index, _, object) in taken {
                if let Some(slot) = self.solvers.get_mut(index) {
                    *slot = Some(object);
                }
            }
            if let (Some(best), Some(&(first, _))) = (result?, blends.first()) {
                // the least of the face's balls (#2401): the face is taken whole once it
                // reaches the face's bound
                let mut least = crate::math::BIG_DOUBLE;
                for &(index, _) in &blends {
                    let id =
                        u32::try_from(index).map_err(|_| invalid("solver index out of range"))?;
                    if let Some(offset) = self.solver(FilletSolverId(id))?.base().surface(0)? {
                        least = least.min(offset.surface.offset_distance().abs());
                    }
                }
                let id = u32::try_from(first).map_err(|_| invalid("solver index out of range"))?;
                out.push((best, FilletObject::Solver(FilletSolverId(id)), least));
            }
        }
        Ok(out)
    }

    fn creation_ball_zone_open(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        radius: Option<f64>,
    ) -> Result<Option<crate::blend::fillet_exact::CollapsedOffset>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        // the other blends' edges, where a chain runs on into the next round (#2403's ball
        // triangle, built only under `fillet_blend_corner`)
        let mut joined = Vec::new();
        if k.fixes.fillet_blend_corner {
            for (index, other) in self.solvers.iter_mut().enumerate() {
                let Some(other) = other.as_mut() else {
                    continue;
                };
                if index == id.0 as usize {
                    continue;
                }
                if let Some(use0) = ball_regular::ops_of(other).solver().edgeuse(0)? {
                    if let Some(e) = crate::topology::edgeuse::p1::edge(self.target, use0)? {
                        joined.push(e);
                    }
                }
            }
        }
        let current = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let (rolls, law) = ball_regular::rolls_a_ball(current);
        if !rolls || law {
            return Ok(None);
        }
        crate::blend::fillet_exact::ball_zone_open_at_an_end(
            ball_regular::ops_of(current),
            k,
            self.target,
            radius,
            &joined,
        )
    }

    fn creation_rail_below_resolution(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
    ) -> Result<Option<crate::blend::fillet_exact::RailBelowResolution>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let current = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let (rolls, law) = ball_regular::rolls_a_ball(current);
        if !rolls || law {
            return Ok(None);
        }
        crate::blend::fillet_exact::rail_below_resolution(
            ball_regular::ops_of(current),
            k,
            self.target,
        )
    }

    fn creation_circle_tops(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
    ) -> Result<Vec<(f64, FilletObject)>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let current = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let mut out = Vec::new();
        for (rho, edge) in fit_size::circle_top_fit(k, self.target, current)? {
            // FitMargin: a thousand of the edge's tolerances
            let margin = 1000.0 * crate::topology::edge::tolerance(self.target, edge)?;
            out.push((
                (rho - margin).max(0.0),
                FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)),
            ));
        }
        Ok(out)
    }

    fn creation_bands_meet(
        &mut self,
        k: &mut Kernel,
        solvers: &[FilletObject],
    ) -> Result<Vec<(f64, FilletObject)>> {
        let mut out = Vec::new();
        for &a in solvers {
            for &b in solvers {
                let (FilletObject::Solver(ia), FilletObject::Solver(ib)) = (a, b) else {
                    return Err(invalid("object is not a solver"));
                };
                if ia == ib {
                    continue;
                }
                let (Some(ea0), Some(_)) = (
                    self.solver(ia)?.base().edgeuse(0)?,
                    self.solver(ib)?.base().edgeuse(0)?,
                ) else {
                    continue;
                };
                let edge = crate::topology::edgeuse::p1::edge(self.target, ea0)?
                    .ok_or_else(|| SolidError::null_pointer(OP))?;
                for ka in 0..2 {
                    for kb in 0..2 {
                        let fa = match self.solver(ia)?.base().edgeuse(ka)? {
                            Some(e) => crate::topology::edgeuse::p1::face(self.target, e)?,
                            None => continue,
                        };
                        let fb = match self.solver(ib)?.base().edgeuse(kb)? {
                            Some(e) => crate::topology::edgeuse::p1::face(self.target, e)?,
                            None => continue,
                        };
                        if fa != fb {
                            continue;
                        }
                        let (Some(mut this), Some(mut other)) = (
                            self.solvers.get_mut(ia.0 as usize).and_then(Option::take),
                            self.solvers.get_mut(ib.0 as usize).and_then(Option::take),
                        ) else {
                            return Err(invalid("solver is not live in this session"));
                        };
                        let meet =
                            fit_size::bands_meet_at(k, self.target, &mut this, &mut other, ka, kb);
                        if let Some(slot) = self.solvers.get_mut(ia.0 as usize) {
                            *slot = Some(this);
                        }
                        if let Some(slot) = self.solvers.get_mut(ib.0 as usize) {
                            *slot = Some(other);
                        }
                        if let Some(meet) = meet? {
                            out.push((
                                meet,
                                FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)),
                            ));
                        }
                    }
                }
            }
        }
        Ok(out)
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
        // `GetFilletSurface()->GetFace()`: the face owning the surface, whichever face
        // of the fillet B-rep that is (fillet surfaces only ever become faces there,
        // `MakeFaceBrep(m_pFilletBrep)`).
        if let Some(intermediate) = self.intermediate.as_ref() {
            for face in intermediate.faces().ids() {
                if owner == self.intermediate_identity(EntityRef::Face(face))? {
                    return Ok(Some(FilletObject::IntermediateTopology(EntityRef::Face(
                        face,
                    ))));
                }
            }
        }
        // A face of another B-rep never owns a fillet surface.
        Err(invalid(
            "fillet surface owned by a face outside the fillet B-rep",
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
        self.stitch_intermediate(k, body, tolerance, &mut DefaultStitchCallback::new())
    }
    fn stitch_body_keeping_fillet_edges(
        &mut self,
        k: &mut Kernel,
        body: FilletObject,
        tolerance: f64,
        corners: &[FilletObject],
    ) -> Result<StitchAttempt> {
        // The patch faces are those MakeFaceBrep made on each corner's surfaces.
        let mut patch_faces = BTreeSet::new();
        for corner in corners {
            // (no corner object: no surfaces)
            let FilletObject::Corner(id) = corner else {
                continue;
            };
            let corner = self
                .corners
                .get(id.0 as usize)
                .ok_or_else(|| invalid("corner is not live in this session"))?;
            patch_faces.extend(corner.surface_faces.iter().copied());
        }
        self.stitch_intermediate(
            k,
            body,
            tolerance,
            &mut FilletPatchStitchCallback { patch_faces },
        )
    }
}

impl SessionObjects<'_> {
    /// `m_pFilletBrep->StitchFaces(...)` of the intermediate body with `callback`'s
    /// keep/delete choice for the glued pairs.
    fn stitch_intermediate<C: StitchCallback>(
        &mut self,
        k: &mut Kernel,
        body: FilletObject,
        tolerance: f64,
        callback: &mut C,
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
            StitchAttempt::from_stitch(crate::topology::brep::p4::stitch_faces_with_callback(
                k,
                body,
                tolerance,
                None,
                false,
                Some(identity),
                Some(&mut edits),
                callback,
            ))
        })?
    }
}

/// The fork's `SmFilletPatchStitchCallback` (tangency-labs/smlib@1072d1b, behind
/// [`crate::kernel::UpstreamFixes::fillet_patch_keeps_fillet_edges`]; WR-546, #1377):
/// the stitch of the fillet body keeps a fillet's own edge where a corner's patch meets
/// it. An N-sided patch's faces are bounded by their surfaces' isoparametric curves,
/// the approximations the patch was fitted to, as far from the fillet's surface as the
/// fillet's exact edge is from the patch; `SmStitchCallback`'s choice between the two
/// is no choice at all. Every other pair takes that choice.
struct FilletPatchStitchCallback {
    /// The corner patch faces of the fillet body.
    patch_faces: BTreeSet<FaceId>,
}

/// Whether an edge's faces are all corner patches, none, or some of each (or it has
/// none at all).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatchSide {
    NoPatch,
    PatchOnly,
    Mixed,
}

impl FilletPatchStitchCallback {
    /// `SideOf`.
    fn side_of(&self, b: &Brep, e: EdgeId) -> Result<PatchSide> {
        let faces = crate::topology::edge::faces(b, e, None)?;
        let patches = faces
            .iter()
            .filter(|&&f| self.patch_faces.contains(&f))
            .count();
        if faces.is_empty() || (patches > 0 && patches < faces.len()) {
            return Ok(PatchSide::Mixed);
        }
        Ok(if patches == 0 {
            PatchSide::NoPatch
        } else {
            PatchSide::PatchOnly
        })
    }
}

impl StitchCallback for FilletPatchStitchCallback {
    fn select_topology_to_keep(
        &mut self,
        k: &Kernel,
        b: &Brep,
        first: EntityRef,
        second: EntityRef,
        dist_between: f64,
    ) -> Result<KeepChoice> {
        let choice = select_topology_to_keep(self, k, b, first, second, dist_between)?;
        let (EntityRef::Edge(edge1), EntityRef::Edge(edge2)) = (first, second) else {
            return Ok(choice);
        };
        let (keep, drop) = match (self.side_of(b, edge1)?, self.side_of(b, edge2)?) {
            (PatchSide::PatchOnly, PatchSide::NoPatch) => (edge2, edge1),
            (PatchSide::NoPatch, PatchSide::PatchOnly) => (edge1, edge2),
            _ => return Ok(choice),
        };

        // How far the kept edge is from the faces of the one it replaces, measured as
        // the default choice measures it.
        let null = || SolidError::null_pointer(OP);
        let keep_curve = crate::topology::edge::curve(b, keep)?.ok_or_else(null)?;
        let keep_interval = crate::topology::edge::interval(b, keep)?;
        let mut max_distance = dist_between;
        for face in crate::topology::edge::faces(b, drop, None)? {
            let surface = crate::topology::face::p2::surface(b, face)?.ok_or_else(null)?;
            let dist = keep_curve
                .surface_max_distance_between(
                    k,
                    &keep_interval,
                    surface,
                    &crate::topology::face::p2::uv_domain(b, face)?,
                    0,
                    None,
                )?
                .max_distance;
            // smos_Max
            if dist > max_distance {
                max_distance = dist;
            }
        }
        Ok(KeepChoice {
            max_distance,
            keep_first: keep == edge1,
        })
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
            extended_surfaces,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner patch is not live"))?;
        corner.calc_corner_geom_with(&mut (), |corner, ()| {
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
                // SmFilletNx1Corner::CreateCornerPatch builds an end patch
                // (CreateEndPatch), whose failure note names the end face.
                let nx1 = matches!(
                    corner.kind,
                    FilletCornerKind::NByOne | FilletCornerKind::Open
                );
                let end_patch = nx1 && corner.edges.len() == 3 && corner.extended_surface_patch;
                // The fork's (WR-697, 79d4763): a Ball's patch is its sphere's
                // (SmBlendCorner::CreateBallPatch)
                if corner
                    .blend
                    .is_some_and(crate::blend::blend_corner::BlendCorner::is_ball)
                {
                    let mut no_note = |_: &FilletCorner| {};
                    let mut env = CornerPatchEnv {
                        k,
                        target,
                        pseudo,
                        solvers: &store,
                        fillet: Some(&mut fillet),
                        note_face_error: &mut no_note,
                    };
                    return crate::blend::blend_corner::ball::create_ball_patch(corner, &mut env);
                }
                // The fork's (61547af, smlib#104): a Setback's patches are its fill's
                // (SmBlendCorner::CreateSetBackPatches)
                if corner
                    .blend
                    .is_some_and(crate::blend::blend_corner::BlendCorner::is_setback)
                {
                    let mut no_note = |_: &FilletCorner| {};
                    let mut env = CornerPatchEnv {
                        k,
                        target,
                        pseudo,
                        solvers: &store,
                        fillet: Some(&mut fillet),
                        note_face_error: &mut no_note,
                    };
                    return crate::blend::blend_corner::setback::create_setback_patches(
                        corner, &mut env,
                    );
                }
                // The fork's (WR-697, 6fd0411): a Mixed corner's two faces are its patches
                // (SmBlendCorner::CreateMixedPatches)
                if corner
                    .blend
                    .is_some_and(crate::blend::blend_corner::BlendCorner::is_mixed)
                {
                    let mut no_note = |_: &FilletCorner| {};
                    let mut env = CornerPatchEnv {
                        k,
                        target,
                        pseudo,
                        solvers: &store,
                        fillet: Some(&mut fillet),
                        note_face_error: &mut no_note,
                    };
                    return crate::blend::blend_corner::mixed::create_mixed_patches(
                        corner, &mut env,
                    );
                }
                // The fork's (WR-697, e647dac): a Swept turn's patch is its torus
                // (SmBlendCorner::CreateSweptPatch)
                if corner
                    .blend
                    .is_some_and(crate::blend::blend_corner::BlendCorner::is_swept)
                {
                    let mut no_note = |_: &FilletCorner| {};
                    let mut env = CornerPatchEnv {
                        k,
                        target,
                        pseudo,
                        solvers: &store,
                        fillet: Some(&mut fillet),
                        note_face_error: &mut no_note,
                    };
                    return crate::blend::blend_corner::swept::create_swept_patch(corner, &mut env);
                }
                // WR-716: the chamfer event's facet is its plane
                if corner
                    .blend
                    .is_some_and(crate::blend::blend_corner::BlendCorner::is_chamfer)
                {
                    let mut no_note = |_: &FilletCorner| {};
                    let mut env = CornerPatchEnv {
                        k,
                        target,
                        pseudo,
                        solvers: &store,
                        fillet: Some(&mut fillet),
                        note_face_error: &mut no_note,
                    };
                    return crate::blend::blend_corner::chamfer::create_chamfer_patch(
                        corner, &mut env,
                    );
                }
                // The fork's (WR-697, 543d25a): a Join has no patch of its own
                // (SmBlendCorner::CreateCornerPatch)
                if corner
                    .blend
                    .is_some_and(crate::blend::blend_corner::BlendCorner::is_join)
                {
                    return Ok(());
                }
                // The fork's (smlib 41a39ec): an End at a smooth vertex's patch is its cap
                // (SmBlendCorner::CreateEndPatchOfEnd)
                if corner.blend.is_some_and(|blend| {
                    blend.is_end() && blend.end == crate::blend::blend_corner::BlendEnd::AtSmooth
                }) {
                    let mut no_note = |_: &FilletCorner| {};
                    let built = {
                        let mut env = CornerPatchEnv {
                            k,
                            target,
                            pseudo,
                            solvers: &store,
                            fillet: Some(&mut fillet),
                            note_face_error: &mut no_note,
                        };
                        crate::blend::blend_corner::end::create_end_cap_patch(corner, &mut env)?
                    };
                    if !built {
                        executive.error.note_corner_error(
                            crate::blend::fillet_geom::p1::FilletErrorType::VertexProblem,
                            corner.corner_type(),
                            crate::blend::fillet_geom::p1::FilStatus::Failure,
                            Some(corner.kind.name()),
                            corner.filleted_vertex().map(|v| {
                                FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))
                            }),
                            [None, None],
                            [None, None],
                            Some(crate::blend::blend_corner::end::SMOOTH_END_CAP_FAILED),
                        );
                        return Err(SolidError::sm_err("SmBlendCorner::CreateEndPatchOfEnd"));
                    }
                    return Ok(());
                }
                let comment = if end_patch {
                    "Fillet Corner Error: Corner End Face Creation Error"
                } else {
                    "Fillet Corner Error: Corner Face Creation Error"
                };
                if corner.kind == FilletCornerKind::NByTwo {
                    // SmFilletNx2Corner::CreateCornerPatch: an end patch (CreateEndPatch,
                    // noting the end face) and the default patch (noting the corner
                    // face). A failed note returns at once, so at most one fires; it
                    // is replayed into the executive's record.
                    type Noted = (FilletCornerType, &'static str, Option<VertexId>);
                    let record =
                        |c: &FilletCorner| (c.corner_type(), c.kind.name(), c.filleted_vertex());
                    let mut face_noted: Option<Noted> = None;
                    let mut end_noted: Option<Noted> = None;
                    let result = {
                        let mut face_note = |c: &FilletCorner| face_noted = Some(record(c));
                        let mut end_note = |c: &FilletCorner| end_noted = Some(record(c));
                        let mut env = CornerPatchEnv {
                            k,
                            target,
                            pseudo,
                            solvers: &store,
                            fillet: Some(&mut fillet),
                            note_face_error: &mut face_note,
                        };
                        corner.create_corner_patch_nx2_with(
                            &mut env,
                            |face| extended_surfaces.get(&face).cloned(),
                            &mut end_note,
                        )
                    };
                    for (noted, comment) in [
                        (
                            end_noted,
                            "Fillet Corner Error: Corner End Face Creation Error",
                        ),
                        (
                            face_noted,
                            "Fillet Corner Error: Corner Face Creation Error",
                        ),
                    ] {
                        let Some((corner_type, name, vertex)) = noted else {
                            continue;
                        };
                        executive.error.note_corner_error(
                            crate::blend::fillet_geom::p1::FilletErrorType::VertexProblem,
                            corner_type,
                            crate::blend::fillet_geom::p1::FilStatus::Failure,
                            Some(name),
                            vertex.map(|v| {
                                FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))
                            }),
                            [None, None], // For now, we don't generally have just two surfaces.
                            [None, None], // For now, we don't generally have just two edgeuses.
                            Some(comment),
                        );
                    }
                    return result;
                }
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
                        Some(comment),
                    );
                };
                if nx1 {
                    let mut no_note = |_: &FilletCorner| {};
                    let (face_note, end_note): (CornerNote<'_>, CornerNote<'_>) = if end_patch {
                        (&mut no_note, &mut note)
                    } else {
                        (&mut note, &mut no_note)
                    };
                    let mut env = CornerPatchEnv {
                        k,
                        target,
                        pseudo,
                        solvers: &store,
                        fillet: Some(&mut fillet),
                        note_face_error: face_note,
                    };
                    return corner.create_corner_patch_nx1_with(
                        &mut env,
                        |face| extended_surfaces.get(&face).cloned(),
                        end_note,
                    );
                }
                let mut env = CornerPatchEnv {
                    k,
                    target,
                    pseudo,
                    solvers: &store,
                    fillet: Some(&mut fillet),
                    note_face_error: &mut note,
                };
                if corner.kind == FilletCornerKind::ThreeByTwoMixed {
                    return corner.create_corner_patch_3x2_with(&mut env);
                }
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

/// The live rollover context: the retained classification context over the corner
/// table, and the executive's corners as `ProcessRollOver`'s `TopologySplit` changes
/// them.
struct SessionRollOver<'a> {
    corners: &'a mut [FilletCorner],
    executive: &'a FilletExecutive<FilletObject>,
}

impl SessionRollOver<'_> {
    fn classification(&self) -> classification::Context<'_> {
        classification::Context {
            corners: self.corners,
        }
    }
}

impl crate::blend::fillet_solver::p5::RailClassificationContext for SessionRollOver<'_> {
    fn rail_curve(&self, edge: EdgeId) -> crate::blend::fillet_classification::FilletClassObject {
        self.classification().rail_curve(edge)
    }
    fn rail_mate_uv_curve(
        &self,
        pseudo: &Brep,
        mate: EdgeuseId,
    ) -> Option<crate::blend::fillet_classification::FilletClassObject> {
        self.classification().rail_mate_uv_curve(pseudo, mate)
    }
    fn classify(
        &mut self,
        k: &Kernel,
        target: &mut Brep,
        pseudo: &Brep,
        face: FaceId,
        class: &mut crate::intersect::curve_class::p1::CurveClassification<
            crate::blend::fillet_classification::FilletClassObject,
        >,
    ) -> Result<()> {
        self.classification()
            .classify(k, target, pseudo, face, class)
    }
    fn corner_type(&self, corner: FilletCornerId) -> Result<FilletCornerType> {
        self.classification().corner_type(corner)
    }
    fn corner_blend_tests(&self, corner: FilletCornerId) -> Result<(bool, bool)> {
        self.classification().corner_blend_tests(corner)
    }
    fn corner_of_vertex(&self, vertex: VertexId) -> Result<Option<FilletCornerId>> {
        Ok(session_corner_of_vertex(self.executive, vertex))
    }
    #[allow(clippy::too_many_arguments)]
    fn classify_free_curve(
        &mut self,
        k: &Kernel,
        target: &mut Brep,
        pseudo: &Brep,
        face: FaceId,
        curve: &crate::curve::curve::Curve,
        interval: crate::geom::extent1d::Extent1d,
        uv_curve: Option<&crate::curve::curve::Curve>,
        zone_tol_3d: f64,
    ) -> Result<
        Vec<
            crate::intersect::curve_class::p1::CurveInterval<
                crate::blend::fillet_classification::FilletClassObject,
            >,
        >,
    > {
        self.classification().classify_free_curve(
            k,
            target,
            pseudo,
            face,
            curve,
            interval,
            uv_curve,
            zone_tol_3d,
        )
    }
    fn corners(&self) -> CornerLookup<'_> {
        CornerLookup::table(self.corners)
    }
    fn with_split_corners(
        &mut self,
        f: &mut dyn FnMut(&mut dyn SplitCorners) -> Result<()>,
    ) -> Result<()> {
        f(&mut SessionSplitCorners {
            executive: self.executive,
            corners: self.corners,
            this: None,
        })
    }
}

impl RailClassificationAccess<FilletObject> for SessionObjects<'_> {
    fn rails_classify(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        adjusted: &mut Vec<FilletObject>,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("rollover object is not an actual solver"));
        };
        let serial = self
            .pseudo
            .brep()
            .notify_serial()
            .ok_or_else(|| invalid("pseudo B-rep has no kernel identity"))?;
        if k.context
            .issued_brep_serial(self.pseudo.brep().notify_identity())
            != Some(serial)
        {
            return Err(invalid("rollover pseudo B-rep belongs to another context"));
        }
        let solver = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("rollover solver is not live"))?;
        let mut context = SessionRollOver {
            corners: &mut self.corners,
            executive,
        };
        let mut actual_adjusted = Vec::new();
        let result = crate::blend::fillet_solver::p1::RetainedRailOwnership::with_context(
            k,
            &mut self.pseudo,
            &mut self.attributes,
            |k, pseudo, ownership| {
                FilletSolver::test_roll_over_with_ops(
                    live_solver_ops(solver),
                    k,
                    self.target,
                    pseudo,
                    &mut context,
                    &mut actual_adjusted,
                    ownership,
                )
            },
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
    fn geometry_overflow_edge(&self, solver: FilletObject) -> Result<Option<FilletObject>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        // the fork adds the edge only when the solver has a first edge use
        let Some(edgeuse) = self.solver(id)?.base().edgeuse(0)? else {
            return Ok(None);
        };
        let edge = crate::topology::edgeuse::p1::edge(self.target, edgeuse)?
            .ok_or_else(|| invalid("solver edgeuse without edge"))?;
        Ok(Some(FilletObject::Topology(
            FilletBody::Target,
            EntityRef::Edge(edge),
        )))
    }
    fn geometry_mitred_corner_edges(
        &self,
        corner: FilletObject,
    ) -> Result<Option<(FilletObject, FilletObject)>> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        let Some(corner) = self.corners.get(id.0 as usize) else {
            return Ok(None);
        };
        if corner.corner_type() != FilletCornerType::NByTwo || !corner.is_mitre() {
            return Ok(None);
        }
        let (first, second) = corner.filleted_edgeuses()?;
        let edge = |edgeuse| -> Result<FilletObject> {
            let edge = crate::topology::edgeuse::p1::edge(self.target, edgeuse)?
                .ok_or_else(|| invalid("corner edgeuse without edge"))?;
            Ok(FilletObject::Topology(
                FilletBody::Target,
                EntityRef::Edge(edge),
            ))
        };
        Ok(Some((edge(first)?, edge(second)?)))
    }
    fn geometry_rails_fit(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        executive: &FilletExecutive<FilletObject>,
    ) -> Result<bool> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let solver = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let mut context = SessionRollOver {
            corners: &mut self.corners,
            executive,
        };
        FilletSolver::rails_fit_their_faces_with_ops(
            live_solver_ops(solver),
            k,
            self.target,
            &mut self.pseudo,
            &mut context,
        )
    }
    fn geometry_rails_fit_judged(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        executive: &FilletExecutive<FilletObject>,
    ) -> Result<crate::blend::fillet_solver::p5::RailsFit> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let solver = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let mut context = SessionRollOver {
            corners: &mut self.corners,
            executive,
        };
        FilletSolver::rails_fit_their_faces_judged_with_ops(
            live_solver_ops(solver),
            k,
            self.target,
            &mut self.pseudo,
            &mut context,
            true,
        )
    }
    fn geometry_end_sections_fit(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        executive: &FilletExecutive<FilletObject>,
    ) -> Result<bool> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let solver = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let mut context = SessionRollOver {
            corners: &mut self.corners,
            executive,
        };
        FilletSolver::end_sections_fit_their_faces_with_ops(
            live_solver_ops(solver),
            k,
            self.target,
            &mut self.pseudo,
            &mut context,
        )
    }
    fn geometry_blend_adds_material(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
    ) -> Result<bool> {
        // sm_BlendAddsMaterial: IsConcaveEdge of the solver's edge
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)) =
            self.geometry_edge(solver)?
        else {
            return Err(invalid("solver edge is not a target edge"));
        };
        crate::blend::fillet_solver::p5::is_concave_edge(k, self.target, edge)
    }
    fn geometry_generator_is_linear(&self, solver: FilletObject) -> Result<bool> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        let solver = self
            .solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        Ok(solver.base().fsg.as_ref().is_some_and(|fsg| {
            fsg.fillet_surface_generator_type() == FilletSurfaceGeneratorType::Linear
        }))
    }
    fn geometry_faces_outlast(&mut self, k: &mut Kernel, solvers: &[FilletObject]) -> Result<bool> {
        use crate::blend::fillet_geom::p1::FilletGeomType;
        use crate::topology::{edgeuse::p1 as eu, loopuse};
        // Each face and blended edge whose blend's rail on that face is a point, and
        // whether that blend's cross section meets the face tangentially.
        let mut point_rails: Vec<(FaceId, EdgeId, bool)> = Vec::new();
        for &solver in solvers {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("object is not a solver"));
            };
            let base = self.solver(id)?.base();
            let fil_eu = base
                .edgeuse(0)?
                .ok_or_else(|| invalid("solver without first edgeuse"))?;
            let fil_edge = eu::edge(self.target, fil_eu)?
                .ok_or_else(|| invalid("solver edgeuse without edge"))?;
            let tangent = base
                .fillet_surface_generator()
                .ok_or_else(|| invalid("solver without surface generator"))?
                .meets_its_faces_tangentially();
            for geom in base.fillet_geoms() {
                let geom = self.pseudo.geom(geom)?;
                if geom.fillet_geom_type() != FilletGeomType::Default {
                    continue;
                }
                for ii in 0..2 {
                    let rail = geom
                        .rail(ii)
                        .ok_or_else(|| invalid("geometry without rail"))?;
                    let curve = crate::topology::edge::curve(self.pseudo.brep(), rail)?
                        .ok_or_else(|| invalid("rail without curve"))?;
                    if curve.is_degenerate(k, base.approx_tol_3d, None)? {
                        let face = self
                            .pseudo
                            .fillet_edge(rail)?
                            .original_face()
                            .ok_or_else(|| invalid("rail without original face"))?;
                        point_rails.push((face, fil_edge, tangent));
                    }
                }
            }
        }
        for &(face, _, _) in &point_rails {
            let Some(fu) = self.target.face(face)?.faceuse else {
                continue;
            };
            for lu in eu::faceuse_loopuses(self.target, fu, false, None)? {
                // The loop closes to a point when every edge of it has a point rail on
                // the face; it ends in that point when some blend there meets the face
                // at an angle.
                let edges = loopuse::edges(self.target, lu, None)?;
                let mut closes_to_a_point = !edges.is_empty();
                let mut all_tangent = true;
                for edge in edges {
                    closes_to_a_point = false;
                    for &(f, e, tangent) in &point_rails {
                        if f == face && Some(e) == edge {
                            closes_to_a_point = true;
                            all_tangent &= tangent;
                        }
                    }
                    if !closes_to_a_point {
                        break;
                    }
                }
                if closes_to_a_point && !all_tangent {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
    fn geometry_ease_set_back_sections(
        &mut self,
        k: &mut Kernel,
        corners: &[FilletObject],
    ) -> Result<()> {
        self.ease_set_back_sections(k, corners)
    }
    fn geometry_bands_clear_other_rails(
        &mut self,
        k: &mut Kernel,
        solvers: &[FilletObject],
    ) -> Result<bool> {
        use crate::blend::fillet_geom::p1::FilletGeomType;
        use crate::geom::global_solver::SolverOperation;
        use crate::intersect::point_class::{PointClassType, PointClassification};
        use crate::topology::edgeuse::p1 as eu;
        // each solver's rails (the face each rolls on and its curve), its filleted edge
        // (with its ends and curve), and the largest solver tolerance
        // (and, with the band between the rail's end vertices, its kept interval)
        let mut rails: Vec<
            Vec<(
                FaceId,
                crate::curve::curve::Curve,
                crate::geom::extent1d::Extent1d,
            )>,
        > = Vec::new();
        let mut edges = Vec::new();
        let mut tol = 0.0;
        for &solver in solvers {
            let FilletObject::Solver(id) = solver else {
                return Err(invalid("object is not a solver"));
            };
            let base = self.solver(id)?.base();
            if base.approx_tol_3d > tol {
                tol = base.approx_tol_3d;
            }
            let mut own = Vec::new();
            for geom in base.fillet_geoms() {
                let geom = self.pseudo.geom(geom)?;
                if geom.fillet_geom_type() != FilletGeomType::Default {
                    continue;
                }
                for ii in 0..2 {
                    let rail = geom
                        .rail(ii)
                        .ok_or_else(|| invalid("geometry without rail"))?;
                    let curve = crate::topology::edge::curve(self.pseudo.brep(), rail)?
                        .ok_or_else(|| invalid("rail without curve"))?
                        .clone();
                    let Some(face) = self.pseudo.fillet_edge(rail)?.original_face() else {
                        continue;
                    };
                    // The rail runs on past its ends (the surfaces are not trimmed yet),
                    // as far as a mitre at a corner needs and beyond: the band is the
                    // rail's part between the vertices that end it, where the corners have
                    // placed them (an end no corner has placed is the rail's own; a rail
                    // whose ends are one vertex is closed and whole). The fork's
                    // tangency-labs/smlib@d548305 (WR-649).
                    let ivl = curve.natural_interval();
                    let pseudo = self.pseudo.brep();
                    let prim = eu::primary_edgeuse(pseudo, rail)?
                        .ok_or_else(|| invalid("rail without primary edgeuse"))?;
                    let mate = eu::mate(pseudo, prim)?
                        .ok_or_else(|| invalid("rail edgeuse without mate"))?;
                    let mut ends = [None; 2];
                    for (slot, use_) in ends.iter_mut().zip([prim, mate]) {
                        let vu = eu::vertexuse(pseudo, use_)?
                            .ok_or_else(|| invalid("rail edgeuse without vertexuse"))?;
                        *slot = crate::topology::vertexuse::vertex(pseudo, vu)?;
                    }
                    let [Some(end0), Some(end1)] = ends else {
                        return Err(invalid("rail end without vertex"));
                    };
                    let mut t_ends = [ivl.min(), ivl.max()];
                    if end0 != end1 {
                        for (t_end, end) in t_ends.iter_mut().zip([end0, end1]) {
                            if !self.pseudo.fillet_vertex(end)?.is_processed() {
                                continue;
                            }
                            let at = crate::topology::vertex::point(pseudo, end)?;
                            let dropped = curve.drop_point(
                                k,
                                &ivl,
                                at,
                                None,
                                crate::math::BIG_DOUBLE,
                                None,
                                SolverOperation::Minimize,
                            )?;
                            if dropped.success {
                                *t_end = dropped.parameter;
                            }
                        }
                    }
                    let [t0, t1] = t_ends;
                    let kept = crate::geom::extent1d::Extent1d::from_min_max(t0, t1);
                    own.push((face, curve, kept));
                }
            }
            rails.push(own);
            let fil_eu = base
                .edgeuse(0)?
                .ok_or_else(|| invalid("solver without first edgeuse"))?;
            let edge = eu::edge(self.target, fil_eu)?
                .ok_or_else(|| invalid("solver edgeuse without edge"))?;
            let ends = [
                crate::topology::edge::start_vertex(self.target, edge)?,
                crate::topology::edge::end_vertex(self.target, edge)?,
            ];
            edges.push((edge, ends));
        }
        let apart = 10.0 * tol;
        for (a, &(edge_a, ends_a)) in edges.iter().enumerate() {
            for (b, &(edge_b, ends_b)) in edges.iter().enumerate() {
                if a == b || edge_a == edge_b {
                    continue;
                }
                if ends_a.iter().any(|v| ends_b.contains(v)) {
                    continue; // a corner trims them where they meet
                }
                let edge_curve_b = self
                    .target
                    .edge(edge_b)?
                    .curve
                    .clone()
                    .ok_or_else(|| invalid("filleted edge without curve"))?;
                let edge_ivl_b = self.target.edge(edge_b)?.interval;
                let (Some(rails_a), Some(rails_b)) = (rails.get(a), rails.get(b)) else {
                    continue;
                };
                for (face_a, curve_a, kept_a) in rails_a {
                    for (face_b, curve_b, _) in rails_b {
                        if face_a != face_b {
                            continue;
                        }
                        let ivl_a = *kept_a;
                        let ivl_b = curve_b.natural_interval();
                        for s in 1..16_u32 {
                            let p =
                                curve_a.evaluate_point(k, ivl_a.evaluate(f64::from(s) / 16.0))?;
                            let face_tol = crate::geom::tol::zone_tol3d(
                                k,
                                crate::topology::face::p2::tolerance(self.target, *face_a)?,
                            );
                            let mut on = PointClassification::<EntityRef>::new(face_tol);
                            crate::query::face::p1::point_3d_classify(
                                k,
                                self.target,
                                *face_a,
                                p,
                                face_tol,
                                true,
                                &mut on,
                                crate::query::face::p1::Point3dClassifyOptions::default(),
                            )?;
                            if on.point_class() == PointClassType::Unknown {
                                continue; // off the face
                            }
                            let on_edge = edge_curve_b.drop_point(
                                k,
                                &edge_ivl_b,
                                p,
                                None,
                                crate::math::BIG_DOUBLE,
                                None,
                                SolverOperation::Minimize,
                            )?;
                            let on_rail = curve_b.drop_point(
                                k,
                                &ivl_b,
                                p,
                                None,
                                crate::math::BIG_DOUBLE,
                                None,
                                SolverOperation::Minimize,
                            )?;
                            let t = on_edge.parameter;
                            if !on_edge.success
                                || !on_rail.success
                                || t <= edge_ivl_b.min()
                                || t >= edge_ivl_b.max()
                            {
                                continue; // beside the band, not across it
                            }
                            let e = edge_curve_b.evaluate_point(k, t)?;
                            let r = curve_b.evaluate_point(k, on_rail.parameter)?;
                            let mut chord = e - r;
                            let width = chord.length();
                            if !matches!(
                                width.partial_cmp(&(2.0 * apart)),
                                Some(core::cmp::Ordering::Greater)
                            ) {
                                continue;
                            }
                            chord *= 1.0 / width;
                            let from_rail = (p - r).dot(chord);
                            if from_rail > apart && from_rail < width - apart {
                                return Ok(false);
                            }
                        }
                    }
                }
            }
        }
        Ok(true)
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
            // the cache is the edge curve's own: its tolerance is the edge's
            let cache = crate::curve::curve::p5::make_curve_cache_with_zone_tol(
                k,
                curve,
                Some(FilletObject::Curve(FilletBody::Target, id)),
                Some(crate::topology::edge::tolerance(self.target, id)?),
            )
            .map_err(|error| {
                if error.is_unported_or_cancelled() {
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
        second_radius: Option<f64>,
    ) -> Result<FilletSolverId> {
        let id = FilletSolverId(
            u32::try_from(self.solvers.len()).map_err(|_| invalid("too many solvers"))?,
        );
        // Reserve the identity before construction writes a geometry pointing at it;
        // a later constructor failure must not let another solver reuse that id.
        self.solvers.push(None);
        let mut solver = constant_radius_from_edgeuse(
            k,
            self.target,
            &mut self.pseudo,
            id,
            approx_tol,
            ang_tol_rad,
            tan_tol_rad,
            radius,
            edgeuse,
            second_radius,
        )?;
        // `new SmConstantRadiusFS(..)`: the plain solver, not the assisted one
        solver.plain = true;
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
                second_radius,
            } => self.allocate_constant_radius_solver(
                k,
                target_edgeuse(edgeuse)?,
                radius,
                approx_tol,
                ang_tol_rad,
                tan_tol_rad,
                second_radius,
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
    fn parameters_edge_is_live(&self, brep: FilletObject, edge: FilletObject) -> Result<bool> {
        let (which, body) = match brep {
            FilletObject::Body(FilletBody::Target) => (FilletBody::Target, &*self.target),
            FilletObject::Body(FilletBody::Pseudo) => (FilletBody::Pseudo, self.pseudo.brep()),
            _ => return Err(invalid("object is not a session body")),
        };
        // (an edge of another body, or not an edge, is not on this body's edge list)
        let FilletObject::Topology(owner, EntityRef::Edge(id)) = edge else {
            return Ok(false);
        };
        Ok(owner == which
            && crate::topology::brep::p1::is_live_topology_member(body, Some(EntityRef::Edge(id))))
    }
    fn parameters_split_blended_vertex_fans(
        &mut self,
        _k: &Kernel,
        brep: FilletObject,
        edges: &[FilletObject],
    ) -> Result<Vec<crate::math::Point3>> {
        // SplitBlendedVertexFans (the fork's, tangency-labs/smlib@e418750): the target only
        let FilletObject::Body(FilletBody::Target) = brep else {
            return Err(invalid("fan split outside the target"));
        };
        let mut done = BTreeSet::new();
        let mut points = Vec::new();
        for &edge in edges {
            let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)) = edge else {
                return Err(invalid("edge is not in the target"));
            };
            let (start, end) = crate::topology::vertex::edge_vertex_pair(self.target, id)?;
            for v in [start, end] {
                let v = v.ok_or_else(|| SolidError::null_pointer(OP))?;
                if !done.insert(v) {
                    continue;
                }
                let made =
                    crate::topology::brep::vertex_fans::split_vertex_by_fans(self.target, v)?;
                if !made.is_empty() {
                    points.push(crate::topology::vertex::point(self.target, v)?);
                }
                done.extend(made);
            }
        }
        Ok(points)
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
            extended_surfaces,
            ..
        } = self;
        let solver = solvers
            .get_mut(solver.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("corner solver is not live"))?;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let env = crate::blend::fillet_corner::topology::Nx1TopologyEnv {
            k,
            target,
            pseudo,
            corner: id,
            solver,
            do_global_merge: &mut executive.options.global_merge,
            extended_surfaces,
            topo_edges: &mut executive.topo_edges,
        };
        let gaps = if corner.kind.corner_type() == FilletCornerType::NByOneClosed {
            corner.make_nx1_closed_corner_topology(env)?;
            None
        } else {
            corner.make_nx1_corner_topology(env)?
        };
        match gaps {
            Some(gaps) => self.nx1_end_gap_checks(k, id, gaps, executive),
            None => Ok(()),
        }
    }
    /// The end of `SmFilletNx1Corner::MakeCornerTopology` for a mixed-convexity end
    /// face spanning several faces: the corner's vertex geometry and the two new side
    /// edges' geometry (statuses ignored, as upstream), then the coincidence of the
    /// kept extended end surface with the other side edge.
    fn nx1_end_gap_checks(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        gaps: crate::blend::fillet_corner::topology::nx1_end::Nx1GapCheck,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        // in this case we can set FilletVertex and SideFilletEdge shapes
        // without having the filletSurface - do so

        // set filletVertex->Positions (fork 66b02de passes this on: SER)
        crate::error::ignore_status(crate::error::passed_on(
            self.calc_nx1_corner_vert_geom(k, id),
            k.fixes.fillet_statuses_pass_on,
        )?)?;

        // set FilletSideEdge->Curves
        for edge in [gaps.edge1, gaps.edge2] {
            let SessionObjects {
                target,
                pseudo,
                solvers,
                corners,
                extended_surfaces,
                ..
            } = &mut *self;
            let corner = corners
                .get(id.0 as usize)
                .ok_or_else(|| invalid("corner is not live"))?;
            let mut lookup = SessionEdgeLookup {
                solvers,
                extended_surfaces,
            };
            let corner_lookup = CornerLookup {
                table: None,
                this: Some((Some(id), corner)),
                exec: None,
            };
            let mut note = ErrorInfo::default();
            let result = pseudo.calc_corner_edge_geom_with_context_and_error_info(
                k,
                edge,
                corner_lookup,
                &mut lookup,
                target,
                Some(&mut note),
            );
            replay_corner_error(&mut executive.error, &note);
            // fork 66b02de passes this on (SER)
            crate::error::ignore_status(crate::error::passed_on(
                result,
                k.fixes.fillet_statuses_pass_on,
            )?)?;
        }

        // When proposing an extended surface
        let Some(face) = gaps.extended_face else {
            return Ok(());
        };
        let other = gaps
            .other_side_edgeuse
            .ok_or_else(|| SolidError::null_pointer("SmFilletNx1Corner::MakeCornerTopology"))?;
        let other_edge = crate::topology::edgeuse::p1::edge(self.target, other)?
            .ok_or_else(|| SolidError::null_pointer("SmFilletNx1Corner::MakeCornerTopology"))?;
        let curve = crate::topology::edge::curve(self.target, other_edge)?
            .ok_or_else(|| SolidError::null_pointer("SmFilletNx1Corner::MakeCornerTopology"))?;
        let interval = crate::topology::edge::interval(self.target, other_edge)?;
        let surface = self
            .extended_surfaces
            .get(&face)
            .ok_or_else(|| SolidError::null_pointer("SmFilletNx1Corner::MakeCornerTopology"))?;
        let corner = self
            .corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        // get extendedSurface/OtherSideEdge coincidence to within tolerance (fork
        // 66b02de passes this on: SER)
        let found = crate::error::ignored(crate::error::passed_on(
            surface.simple_coincidence_checker::<crate::topology::ObjectHandle>(
                k,
                &surface.natural_uv_domain_with_nurb(k)?,
                curve,
                &interval,
                corner.approx_tol_3d,
                false,
                [None, None],
            ),
            k.fixes.fillet_statuses_pass_on,
        )?)?
        .flatten()
        .is_some();
        // when extendedSurface/OtherSideEdge are not coincident
        if !found {
            // can't define endFace->Surface as a sideFace->ExtendedSurface
            self.corners
                .get_mut(id.0 as usize)
                .ok_or_else(|| invalid("corner is not live"))?
                .extended_surface_patch = false;
        }
        Ok(())
    }
    fn make_nx2_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        // The fork's (WR-697, 79d4763): SmBlendCorner::MakeCornerTopology builds a Ball
        // its own way (MakeBallTopology), with the solver of each ring edge use (or of
        // its mate)
        if self
            .corners
            .get(id.0 as usize)
            .and_then(|corner| corner.blend)
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_ball)
        {
            return self.make_ball_corner_topology(k, id, executive);
        }
        // The fork's (61547af, smlib#104): SmBlendCorner::MakeCornerTopology builds a
        // Setback its own way (MakeSetBackTopology)
        if self
            .corners
            .get(id.0 as usize)
            .and_then(|corner| corner.blend)
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_setback)
        {
            return self.make_setback_corner_topology(k, id, executive);
        }
        // The fork's (WR-697, 543d25a): SmBlendCorner::MakeCornerTopology builds a Join on
        // its shared section (MakeJoinTopology)
        if self
            .corners
            .get(id.0 as usize)
            .and_then(|corner| corner.blend)
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_join)
        {
            return self.make_join_corner_topology(k, id, executive);
        }
        // The fork's (smlib 41a39ec): SmBlendCorner::MakeCornerTopology builds an End at a
        // smooth vertex on the plane square to its edge (MakeEndAtSmoothVertex)
        if self
            .corners
            .get(id.0 as usize)
            .and_then(|corner| corner.blend)
            .is_some_and(|blend| {
                blend.is_end() && blend.end == crate::blend::blend_corner::BlendEnd::AtSmooth
            })
        {
            return self.make_end_at_smooth_corner_topology(k, id, executive);
        }
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
    /// The fork's `SmBlendCorner::MakeBallTopology` (WR-697) with the solver of each of
    /// the corner's ring edge uses, else of its mate (`GetFilletSolverOfEdgeuse`).
    fn make_ball_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let all = self
            .corners
            .get(id.0 as usize)
            .and_then(|corner| corner.all_eus.clone())
            .ok_or_else(|| SolidError::null_pointer("SmBlendCorner::MakeBallTopology"))?;
        let mut solver_ids = vec![FilletSolverId(0); all.len()];
        for (slot, index) in solver_ids.iter_mut().zip(0..) {
            let eu = all
                .get(index)
                .copied()
                .ok_or_else(|| SolidError::sm_err("SmBlendCorner::MakeBallTopology"))?;
            let of = |eu: EdgeuseId| {
                let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(eu));
                match executive.fillet_solver_of_edgeuse(object) {
                    Some(FilletObject::Solver(solver)) => Some(solver),
                    _ => None,
                }
            };
            let found = match of(eu) {
                Some(solver) => Some(solver),
                None => crate::topology::edgeuse::p1::mate(self.target, eu)?.and_then(of),
            };
            *slot =
                found.ok_or_else(|| SolidError::null_pointer("SmBlendCorner::MakeBallTopology"))?;
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
        let solvers = solver_ids
            .iter()
            .map(|&id| live(id))
            .collect::<Result<Vec<_>>>()?;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        corner.make_ball_topology(crate::blend::fillet_corner::topology::BallTopologyEnv {
            k,
            target,
            pseudo,
            corner: id,
            solvers,
        })
    }
    /// The fork's `SmBlendCorner::MakeJoinTopology` (WR-697, 543d25a) with the solvers of
    /// the corner's blended edge uses (`GetFilletSolverOfEdgeuse`) resolved first; its
    /// refusal noted at the vertex (`SM_FIL_FAILURE`, `SM_FILERR_NO_ROOM` with the fit 0,
    /// 41a39ec).
    fn make_join_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        use crate::blend::fillet_corner::topology::join::{
            JoinTopology, JoinTopologyEnv, JOIN_ACROSS_EDGES,
        };
        let corner = self
            .corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let mut solver_eu_solvers = Vec::new();
        for &eu in corner.solver_eus.as_deref().unwrap_or_default() {
            let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(eu));
            solver_eu_solvers.push(match executive.fillet_solver_of_edgeuse(object) {
                Some(FilletObject::Solver(solver)) => Some(solver),
                None => None,
                Some(_) => return Err(invalid("edge use does not name an actual solver")),
            });
        }
        let vertex = corner.filleted_vertex();
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let built = corner.make_join_topology(JoinTopologyEnv {
            k,
            target,
            pseudo,
            corner: id,
            solvers,
            solver_eu_solvers,
        })?;
        if built == JoinTopology::RefusedAcrossEdges {
            let location = match vertex {
                Some(v) => crate::topology::vertex::point(target, v).ok(),
                None => None,
            };
            // No size builds it: the edges between the blends do not change with it.
            // Refused as too large with no size that builds (41a39ec).
            executive.error.note_vertex_error(
                vertex.map(|v| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))),
                location,
                crate::blend::fillet_geom::p1::FilStatus::Failure,
                crate::blend::fillet_geom::p1::FilletErrorType::NoRoom,
                Some(JOIN_ACROSS_EDGES),
            );
            let first = corners
                .get(id.0 as usize)
                .and_then(|c| c.solver_eus.as_deref())
                .and_then(|eus| eus.first().copied())
                .ok_or_else(|| SolidError::null_pointer("SmBlendCorner::MakeJoinTopology"))?;
            let edge = crate::topology::edgeuse::p1::edge(target, first)?;
            executive.note_fit_radius(
                0.0,
                edge.map(|e| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e))),
                crate::blend::fillet_executive::p3::FitKind::Radius,
            );
            return Err(SolidError::sm_err("SmBlendCorner::MakeJoinTopology"));
        }
        Ok(())
    }

    /// The fork's `SmBlendCorner::MakeEndAtSmoothVertex` (41a39ec) with the solver of the
    /// blended edge use (`GetFilletSolverOfEdgeuse`) resolved first.
    fn make_end_at_smooth_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        use crate::blend::fillet_corner::topology::end_smooth::EndAtSmoothTopologyEnv;
        let corner = self
            .corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let solver_eu_solver = match corner.solver_eus.as_deref().and_then(|eus| eus.first()) {
            Some(&eu) => {
                let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(eu));
                match executive.fillet_solver_of_edgeuse(object) {
                    Some(FilletObject::Solver(solver)) => Some(solver),
                    None => None,
                    Some(_) => return Err(invalid("edge use does not name an actual solver")),
                }
            }
            None => None,
        };
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        corner.make_end_at_smooth_vertex(EndAtSmoothTopologyEnv {
            k,
            target,
            pseudo,
            corner: id,
            solvers,
            solver_eu_solver,
        })
    }

    /// The fork's `SmBlendCorner::MakeSetBackTopology` (61547af, smlib#104) with the solver
    /// of each of the corner's ring edge uses, else of its mate.
    fn make_setback_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            set_back_fills,
            setback_refusal,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let refused = {
            let lookup = SweptSessionSolvers {
                solvers: solvers.as_slice(),
                executive: Some(&*executive),
            };
            crate::blend::blend_corner::setback::make_setback_topology(
                corner,
                crate::blend::blend_corner::setback::SetbackTopologyEnv {
                    k,
                    target,
                    pseudo,
                    corner: id,
                    solvers: &lookup,
                    fills: set_back_fills,
                },
            )?
        };
        if let Err(refusal) = refused {
            note_setback_refusal(executive, target, corner.filleted_vertex(), &refusal);
            *setback_refusal = refusal.refusal;
            return Err(setback_refused(
                "SmBlendCorner::MakeSetBackTopology",
                &refusal,
            ));
        }
        Ok(())
    }
    /// `MakeCornerTopology` of a 1x1, 2x2, 4x3, open or degenerate corner, with the
    /// solvers of its edge uses (`GetFilletSolverOfEdgeuse`) resolved first.
    fn make_end_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let corner = self
            .corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let solver_of = |eu: EdgeuseId| {
            let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(eu));
            match executive.fillet_solver_of_edgeuse(object) {
                Some(FilletObject::Solver(solver)) => Ok(Some(solver)),
                None => Ok(None),
                Some(_) => Err(invalid("edge use does not name an actual solver")),
            }
        };
        let resolve = |eus: Option<&[EdgeuseId]>| -> Result<Vec<Option<FilletSolverId>>> {
            eus.unwrap_or_default()
                .iter()
                .map(|&eu| solver_of(eu))
                .collect()
        };
        let solver_eu_solvers = resolve(corner.solver_eus.as_deref())?;
        let all_eu_solvers = if corner.kind.corner_type() == FilletCornerType::FourByThree {
            resolve(corner.all_eus.as_deref())?
        } else {
            Vec::new()
        };
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let env = crate::blend::fillet_corner::topology::end_corners::EndCornerTopologyEnv {
            k,
            target,
            pseudo,
            corner: id,
            solvers,
            solver_eu_solvers,
            all_eu_solvers,
        };
        match corner.kind.corner_type() {
            FilletCornerType::OneByOne => corner.make_1x1_corner_topology(env),
            FilletCornerType::TwoByTwo => corner.make_2x2_corner_topology(env),
            FilletCornerType::FourByThree => corner.make_4x3_corner_topology(env),
            FilletCornerType::Open => corner.make_open_corner_topology(env),
            FilletCornerType::Degenerate => corner.make_degenerate_corner_topology(env),
            _ => Err(invalid("not a 1x1, 2x2, 4x3, open or degenerate corner")),
        }
    }
    fn make_3x2_mixed_corner_topology(
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
                    "SmFillet3x2MixedConvexityCorner::MakeCornerTopology",
                ));
            };
            *slot = solver;
        }
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
        corner.make_3x2_mixed_corner_topology(
            crate::blend::fillet_corner::topology::mixed_3x2::Mixed3x2TopologyEnv {
                k,
                target,
                pseudo,
                corner: id,
                solvers,
                solver_ids,
                extended_surfaces,
            },
        )
    }
    fn make_convex_nxn_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        const NATIVE: &str = "SmFilletConvexNxNCorner::MakeCornerTopology";
        // WR-716: the chamfer event's Mitre is the bevel's topology (the crossings, the
        // centre M, the edges to it), its Facet and its corner patch the ring's (the
        // crossings and the sides between them), whatever the caller set
        if let Some(corner) = self.corners.get_mut(id.0 as usize) {
            if let Some(event) = corner.chamfer_corner.as_ref() {
                corner.bevel = event.definition.is_mitre();
            }
        }
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
        self.calc_nx1_corner_vert_geom_rolling(k, id, false)
    }

    /// [`Self::calc_nx1_corner_vert_geom`]; `rolled_end` as [`live_corner_vertices_geom`]'s.
    fn calc_nx1_corner_vert_geom_rolling(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        rolled_end: bool,
    ) -> Result<()> {
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            extended_surfaces,
            ..
        } = self;
        live_corner_vertices_geom(
            k,
            target,
            pseudo,
            solvers,
            corners,
            extended_surfaces,
            id,
            rolled_end,
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
        // by the corner's construction (its class): a blend corner's End builds the Nx1,
        // open or degenerate corner's moved code (the fork's 53c47b8), a Join is dispatched
        // in the Nx2 corner's (WR-697)
        match self.corner_object(corner)?.kind.corner_type() {
            FilletCornerType::NByOne | FilletCornerType::NByOneClosed => {
                self.make_nx1_corner_topology(k, id, executive)
            }
            FilletCornerType::NByTwo => self.make_nx2_corner_topology(k, id, executive),
            FilletCornerType::NByNConvex => self.make_convex_nxn_corner_topology(k, id, executive),
            FilletCornerType::NByNConcave => {
                self.make_concave_nxn_corner_topology(k, id, executive)
            }
            FilletCornerType::ThreeByTwoMixed => {
                self.make_3x2_mixed_corner_topology(k, id, executive)
            }
            FilletCornerType::OneByOne
            | FilletCornerType::TwoByTwo
            | FilletCornerType::FourByThree
            | FilletCornerType::Open
            | FilletCornerType::Degenerate => self.make_end_corner_topology(k, id, executive),
            // Abstract upstream (pure virtual): no corner object has these types.
            FilletCornerType::Unknown | FilletCornerType::NByN => {
                Err(invalid("abstract corner type has no MakeCornerTopology"))
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
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        // Not the fork's (#2406): an End at a smooth vertex that can wait for its rails'
        // rollover (the rollover follows the corners' edges, not in a cliff overflow)
        let rolled_end = k.fixes.fillet_blend_corner && {
            let rolls_over = executive.options.classification && !executive.cliff_overflow;
            let SessionObjects {
                target,
                pseudo,
                solvers,
                corners,
                ..
            } = self;
            let corner = corners
                .get(id.0 as usize)
                .ok_or_else(|| invalid("corner is not live"))?;
            corner
                .blend
                .is_some_and(crate::blend::blend_corner::BlendCorner::is_end)
                && crate::blend::blend_corner::end::end_can_await_rollover(
                    k,
                    target,
                    pseudo,
                    corner,
                    &SweptSessionSolvers {
                        solvers: solvers.as_slice(),
                        executive: Some(&*executive),
                    },
                    rolls_over,
                )?
        };
        self.calc_nx1_corner_vert_geom_rolling(k, id, rolled_end)
    }
}

impl CornerAdjustmentAccess<FilletObject> for SessionObjects<'_> {
    fn adjustment_check_degeneracy(
        &mut self,
        k: &mut Kernel,
        corner: FilletObject,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<bool> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let solvers = &**solvers;
        corner.check_and_fix_degeneracy_with(
            k,
            target,
            pseudo,
            |e| {
                executive.remove_from_topo_edges(Some(FilletObject::Topology(
                    FilletBody::Pseudo,
                    EntityRef::Edge(e),
                )));
            },
            |solver| {
                solvers
                    .get(solver.0 as usize)
                    .and_then(Option::as_ref)
                    .map(SolverObject::base)
                    .ok_or_else(|| invalid("corner solver is not live"))
            },
        )
    }
    fn adjustment_topology(
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
            corners,
            solvers,
            extended_surfaces,
            ..
        } = self;
        // The running corner is taken out of its slot so that the big-radius
        // conversion can reach the other corners (and add one) meanwhile.
        let slot = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let placeholder = FilletCorner::new(FilletCornerKind::Open, None, None, None, 0.0, 0.0);
        let mut corner = std::mem::replace(slot, placeholder);
        let mut context = SessionNx2Adjust {
            k,
            target,
            pseudo,
            corner: id,
            solvers: solvers.as_mut_slice(),
            corners,
            extended_surfaces,
            executive,
        };
        let result = corner.adjust_topology_with(
            &mut context,
            FilletCorner::adjust_topology_nx2_with,
            // SmFilletConcaveNxNCorner inherits SmFilletConvexNxNCorner::AdjustTopology.
            |corner, context| {
                let solvers = &*context.solvers;
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
        );
        if let Some(slot) = corners.get_mut(id.0 as usize) {
            *slot = corner;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::fillet_geom::p1::{FilStatus, FilletErrorType};
    use crate::curve::curve::Curve;
    use crate::curve::line::Line;
    use crate::geom::tol::USE_DEFAULT;
    use crate::math::Point3;
    use crate::topology::brep::p2::ArenaNotifyState;
    use crate::topology::brep::p8::{create_planar_face_with_3d_curves, FaceBuildEnv};
    use crate::topology::loopuse;
    use crate::topology::topology_traverser::Marks;

    /// A pentagonal pyramid: its apex has five edges, so filleting three of them is a
    /// corner `SmFilletCorner::Create` has no type for (three of five, not the 3x2 or
    /// 4x3 cases).
    fn pentagonal_pyramid(k: &mut Kernel) -> Brep {
        let mut brep = crate::topology::brep::p3::new_brep(k, USE_DEFAULT, USE_DEFAULT).unwrap();
        let region = brep.infinite_region();
        let mut state = ArenaNotifyState::new(&brep);
        let mut env = state.env(None);
        let mut marks = Marks::default();
        let this_face = |face: FaceId| loopuse::notify_ref(EntityRef::Face(face));
        let mut fenv = FaceBuildEnv {
            env: &mut env,
            marks: &mut marks,
            this_face: &this_face,
        };
        let base: Vec<Point3> = (0..5)
            .map(|ii| {
                let angle = std::f64::consts::TAU * f64::from(ii) / 5.0;
                Point3::new(10.0 * angle.cos(), 10.0 * angle.sin(), 0.0)
            })
            .collect();
        let apex = Point3::new(0.0, 0.0, 15.0);
        let line =
            |a: Point3, b: Point3| Curve::from(Line::create_line_segment(3, a, b, None).unwrap());

        let mut base_curves = Vec::new();
        for ii in 0..base.len() {
            base_curves.push(line(base[ii], base[(ii + 1) % base.len()]));
        }
        create_planar_face_with_3d_curves(k, &mut brep, region, &base_curves, 1.0e-5, &mut fenv)
            .unwrap();
        for ii in 0..base.len() {
            let p0 = base[ii];
            let p1 = base[(ii + 1) % base.len()];
            let side = vec![line(p0, p1), line(p1, apex), line(apex, p0)];
            create_planar_face_with_3d_curves(k, &mut brep, region, &side, 1.0e-5, &mut fenv)
                .unwrap();
        }
        crate::topology::brep::p5::stitch_and_orient(
            k,
            &mut brep,
            &crate::topology::brep::p5::StitchAndOrientOptions::default(),
        )
        .unwrap();
        brep.refresh_cached_topology();
        brep
    }

    #[test]
    fn create_corners_consults_the_vertex_diagnostics_fix() {
        for fix in [false, true] {
            let mut k = Kernel::default();
            k.fixes.fillet_vertex_error_diagnostics = fix;
            let mut body = pentagonal_pyramid(&mut k);
            let apex = crate::topology::brep::p1::vertices(&body, None)
                .unwrap()
                .into_iter()
                .find(|vertex| crate::topology::vertex::point(&body, *vertex).unwrap().z > 1.0)
                .unwrap();
            let side_edges = crate::topology::brep::p1::edges(&body, None)
                .unwrap()
                .into_iter()
                .filter(|edge| {
                    crate::topology::edge::vertices(&body, *edge, None)
                        .unwrap()
                        .iter()
                        .flatten()
                        .any(|vertex| *vertex == apex)
                })
                .collect::<Vec<_>>();
            assert_eq!(side_edges.len(), 5);
            let apex_point = crate::topology::vertex::point(&body, apex).unwrap();
            let mut session = FilletSession::new(&mut k, &mut body).unwrap();
            session
                .prepare_constant_radius_edges(&mut k, &side_edges[..3], 0.5, 1, 1, 1.0)
                .unwrap();
            session.create_corners(&k, None).unwrap();
            // The three base corners are made; the apex keeps its pending entry.
            assert_eq!(session.fillet_corners().unwrap().len(), 3);
            let error = session.error_info();
            if fix {
                assert_eq!(error.code, FilletErrorType::CornerUnsupported as i32);
                assert_eq!(error.status, FilStatus::Failure as i32);
                assert_eq!(error.comment, "Corner case not implemented");
                assert_eq!(
                    error.vertex,
                    Some(FilletObject::Topology(
                        FilletBody::Target,
                        EntityRef::Vertex(apex)
                    ))
                );
                assert_eq!(error.location, Some(apex_point));
            } else {
                // Pinned default: the vertex note records nothing.
                assert_eq!(error.code, 0);
                assert_eq!(error.vertex, None);
                assert_eq!(error.location, None);
                assert_eq!(error.comment, "<no comment>\n");
            }
        }
    }

    #[test]
    fn do_fillet_keeps_the_missing_corner_note_or_the_same_later_failure() {
        for (fix, stale_note) in [(false, false), (false, true), (true, false)] {
            let mut k = Kernel::default();
            k.fixes.fillet_vertex_error_diagnostics = fix;
            let mut body = pentagonal_pyramid(&mut k);
            let apex = crate::topology::brep::p1::vertices(&body, None)
                .unwrap()
                .into_iter()
                .find(|vertex| crate::topology::vertex::point(&body, *vertex).unwrap().z > 1.0)
                .unwrap();
            let side_edges = crate::topology::brep::p1::edges(&body, None)
                .unwrap()
                .into_iter()
                .filter(|edge| {
                    crate::topology::edge::vertices(&body, *edge, None)
                        .unwrap()
                        .iter()
                        .flatten()
                        .any(|vertex| *vertex == apex)
                })
                .collect::<Vec<_>>();
            let apex_point = crate::topology::vertex::point(&body, apex).unwrap();
            let mut session = FilletSession::new(&mut k, &mut body).unwrap();
            session
                .prepare_constant_radius_edges(&mut k, &side_edges[..3], 0.5, 1, 1, 1.0)
                .unwrap();
            if stale_note {
                // Disabling the policy must not preserve a note from an earlier
                // opt-in operation instead of the current stage's own diagnostic.
                session.executive.error.code = FilletErrorType::CornerUnsupported as i32;
            }
            let result = session.do_fillet(&mut k);
            // The later stage's failure is the same with the fix on and off.
            assert!(matches!(
                result,
                Err(SolidError::Upstream { code: 1001, .. })
            ));
            let error = session.error_info();
            if fix {
                assert_eq!(error.code, FilletErrorType::CornerUnsupported as i32);
                assert_eq!(error.status, FilStatus::Failure as i32);
                assert_eq!(error.comment, "Corner case not implemented");
                assert_eq!(
                    error.vertex,
                    Some(FilletObject::Topology(
                        FilletBody::Target,
                        EntityRef::Vertex(apex)
                    ))
                );
                assert_eq!(error.location, Some(apex_point));
            } else {
                // Pinned default: the later stage's own error info, unchanged.
                assert_eq!((error.code, error.status), (4, 10));
                assert_eq!(error.comment, "Fillet Surface Computation Failure");
                assert_eq!((error.vertex, error.location), (None, None));
            }
        }
    }
}

#[cfg(test)]
mod attribute_seed_tests {
    use super::*;
    use crate::geom::attribute::ObjectKey;
    use crate::math::Point3;
    use crate::primitive::api_primitives::create_box;

    /// A second base whose seed fails (an attribute list naming no object) fails the
    /// session's construction and leaves both bodies' attributes where they were: the
    /// target's were not taken before every seed succeeded.
    #[test]
    fn failed_seed_leaves_both_bodies_attributes() {
        let mut k = Kernel::default();
        let mut target = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
        let face = crate::topology::brep::p1::faces(&target, None)
            .unwrap()
            .first()
            .copied()
            .unwrap();
        target
            .add_attribute(
                EntityRef::Face(face),
                Attribute::long(10_007, 7, Behavior::Copy),
            )
            .unwrap();
        let mut second = create_box(&mut k, Point3::new(5.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let mut store = AttributeStore::new();
        let handle = store
            .insert(Attribute::long(10_008, 8, Behavior::Copy))
            .unwrap();
        // key 0 names no object (`loopuse::entity_of_notify_key`)
        store.add_attribute(ObjectKey(0), handle).unwrap();
        second.set_attributes(store);
        let target_before = target.attribute_store().clone();
        let second_before = second.attribute_store().clone();

        assert!(FilletSession::with_second_base(&mut k, &mut target, Some(&mut second)).is_err());
        assert_eq!(target.attribute_store(), &target_before);
        assert_eq!(second.attribute_store(), &second_before);
        assert_eq!(target.entity_attributes(EntityRef::Face(face)).len(), 1);
    }

    /// A hand-back that fails (the session store lists an attribute that was deleted,
    /// so `body_attributes` errors) puts each body's store from before the session
    /// back, rather than leaving the bodies with no attributes.
    #[test]
    fn failed_hand_back_restores_both_bodies_attributes() {
        let mut k = Kernel::default();
        let mut target = create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
        let face = crate::topology::brep::p1::faces(&target, None)
            .unwrap()
            .first()
            .copied()
            .unwrap();
        target
            .add_attribute(
                EntityRef::Face(face),
                Attribute::long(10_007, 7, Behavior::Copy),
            )
            .unwrap();
        let mut second = create_box(&mut k, Point3::new(5.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let second_face = crate::topology::brep::p1::faces(&second, None)
            .unwrap()
            .first()
            .copied()
            .unwrap();
        second
            .add_attribute(
                EntityRef::Face(second_face),
                Attribute::long(10_008, 8, Behavior::Copy),
            )
            .unwrap();
        let target_before = target.attribute_store().clone();
        let second_before = second.attribute_store().clone();

        {
            let mut session =
                FilletSession::with_second_base(&mut k, &mut target, Some(&mut second)).unwrap();
            let store = &mut session.objects.attributes;
            let planted = store
                .insert(Attribute::long(10_009, 9, Behavior::Copy))
                .unwrap();
            // key 0 names no object (`loopuse::entity_of_notify_key`), so the hand-back
            // clears its list; dropping its only user deletes the attribute but leaves
            // the list naming it, and clearing that list then fails.
            store.add_attribute(ObjectKey(0), planted).unwrap();
            assert!(store.remove_user(planted, ObjectKey(0), false).unwrap());
            assert!(
                body_attributes(store, session.objects.target, session.objects.target_serial)
                    .is_err()
            );
        }
        assert_eq!(target.attribute_store(), &target_before);
        assert_eq!(second.attribute_store(), &second_before);
        assert_eq!(target.entity_attributes(EntityRef::Face(face)).len(), 1);
        assert_eq!(
            second.entity_attributes(EntityRef::Face(second_face)).len(),
            1
        );
    }
}
