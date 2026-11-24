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
//! Live corner stages dispatch `NByOne`, `NByTwo` and `NxN` corners
//! to the ported corner code. One vertex dispatch (`live_corner_vert_geom`) serves every corner kind;
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
//! result for the remaining fillet execution stages. Band checks recognize constrained
//! closest-point endpoints within the existing solver tolerance in geometric space.
//! No recursion.
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
use super::p2::{EdgeMapAccess, FilletParameters, FilletParametersAccess, RunOn, SolverSpec};
use super::p3::{
    BandFit, BrepCreationAccess, CornerAdjustmentAccess, CornerEdgeAccess, CornerVertexAccess,
    CornerView, ErrorInfo, FaceConstructionAccess, FaceShrinkAccess, FaceStitchAccess,
    FilletExecutive, OpenEnd, RailClassificationAccess, RailTrimAccess, SolverGeometryAccess,
    StitchAttempt, VertexEntry,
};
use crate::blend::fillet_corner::p1::{EndRibDrop, LawIntervalSolvers};
use crate::blend::fillet_corner::p3::{
    CornerFilletBrep, CornerPatchEnv, CornerSolverStore, ExecutiveErrorObject, FilletCorner,
    FilletCornerKind, Nx2AdjustContext,
};
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilletBrep, FilletCornerId, FilletCornerType, FilletSolverId, FilletVertexType,
};
use crate::blend::fillet_geom::p3::SplitCorners;
use crate::blend::fillet_geom::p4::rail_crossing_nearest;
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
use crate::geom::extent3d::Extent3d;
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
mod blend_face_log;
mod classification;
mod cliff_overflow;
mod concave_nxn;
mod corner_marks;
mod ease;
mod edge_refusals;
pub(crate) mod finish;
pub mod fit_size;
mod keep_features;
mod law_corners;
mod original_merge;
mod original_trim;
mod overflow_first;
mod piecewise;
mod planar_bands;
mod surface;
mod tangent_rollover;
mod trace_reach;
mod trim_at_creases;
mod two_offsets;
pub use original_trim::{OriginalSurfaceDeletion, OriginalSurfaceSelection};

const OP: &str = "blend::fillet_executive::session";

/// The session's note at a bevel corner of three chamfers of both convexities.
const MIXED_CHAMFER_CORNER: &str = "Fillet Error: chamfers of convex and of concave edges meet at the vertex; their mitre is not built yet";
/// The same, as the refusing step's reason.
const MIXED_CHAMFER_CORNER_REASON: &str =
    "chamfers of convex and of concave edges meet at the vertex";

/// The tangency within which the band check takes two blended edges for one tangent chain
/// (`chains_among`), radians: 10 degrees, as a chamfer's chain (`two_offsets`) and an
/// edge's continuation (`edge_refusals`) take it.
const BAND_CHAIN_TANGENCY: f64 = 10.0 * (crate::math::PI / 180.0);

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// Why a blended edge use has its fillet solver: the executive loads one for each blended
/// edge use before it classifies or builds a corner, and every edge use a corner names a
/// solver for is a blended one.
const SOLVER_OF_BLENDED_EDGEUSE: &str = "the executive loads a fillet solver for each blended edge use before it classifies or builds a corner, and the edge uses a corner takes solvers from are blended ones";

/// Why a fillet solver has its first edge use: each is built on the edge use it blends.
/// Why an Nx1 corner's gap has its other-side edge use and its face's extended surface once
/// it names an extended face: the corner's end sets all three together, when it builds the
/// extended surface and adds it to the session.
const GAP_EXTENSION_SET_TOGETHER: &str = "a gap's extended face, the edge use across it and the face's extended surface are set together, when the corner's end builds the extended surface and adds it";

/// Why a join corner holds the solver edge use the refusal names: the join is built on the
/// corner's solver edge uses, one per blend it joins, and names one of them.
const JOIN_EDGEUSES: &str = "a join corner holds its solver edge uses, one per blend it joins, and the refusal names one of them";

const SOLVER_ON_ITS_EDGEUSE: &str = "each fillet solver is built on the edge use it blends";

/// Why a corner id names a corner: the session hands out ids as indices of its corner
/// table, which only grows.
const CORNER_OF_ITS_ID: &str =
    "a corner id is an index of the session's corner table, which only grows";

/// A reviewed defect (section 6 of `docs/failure-codes-and-reproduction.md`): `why` says why
/// the missing thing is there. Status `SM_ERR_NULL_POINTER`, the fork's `NER`, as the phrase
/// cause it replaces had.
#[track_caller]
fn null_defect(op: &'static str, why: &'static str) -> SolidError {
    crate::error::Failure::new(op, crate::error::blend::BlendFailure::Defect { why })
        .with_status(crate::error::SM_ERR_NULL_POINTER)
        .into_error()
}

/// A reviewed defect, status `SM_ERR` (the default of a defect, as the cause it replaces had).
#[track_caller]
fn defect(op: &'static str, why: &'static str) -> SolidError {
    crate::error::Failure::new(op, crate::error::blend::BlendFailure::Defect { why }).into_error()
}

/// The target's edge use `id` without its edge: caller-body state.
fn edgeuse_without_edge(op: &'static str, id: EdgeuseId) -> SolidError {
    SolidError::blend_at(
        op,
        crate::error::blend::BlendFailure::EdgeUseWithoutEdge,
        EntityRef::Edgeuse(id),
    )
}

/// The target's edge `id` without a curve: caller-body state.
fn edge_without_curve(op: &'static str, id: EdgeId) -> SolidError {
    SolidError::blend_at(
        op,
        crate::error::blend::BlendFailure::EdgeWithoutCurve,
        EntityRef::Edge(id),
    )
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

    /// Not upstream (#15906): the fixed offset distances of a constant-radius solver's two
    /// surfaces ([`ConstantRadiusFs::oriented_offsets`]); `None` for a solver whose offsets
    /// change along the blend (constant distance: its offset is solved at each section;
    /// variable radius: its law's).
    ///
    /// # Errors
    /// Those of [`ConstantRadiusFs::oriented_offsets`].
    pub fn constant_offsets(&self) -> Result<Option<[f64; 2]>> {
        match self {
            Self::ConstantRadius(solver) => Ok(Some(solver.oriented_offsets()?)),
            Self::SurfaceSurface(solver) => Ok(Some(solver.base.oriented_offsets()?)),
            Self::ConstantDistance(_) | Self::VariableRadius(_) => Ok(None),
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

/// WR-459: the target's edges before filleting, each with its end vertices
/// (`edge::vertices`): the clean-up keeps an edge by its id only while it has those ends.
type CleanupKeptEdges = BTreeMap<EdgeId, Vec<Option<VertexId>>>;

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
    /// WR-459: the target's edges, each with its end vertices, and faces before filleting,
    /// kept by the clean-up.
    cleanup_kept: Option<(CleanupKeptEdges, BTreeSet<FaceId>)>,
    /// WR-661: the target's boundary (lamina edges) and faces before filleting, which an
    /// open result must keep to be kept ([`finish::OpenInput`]).
    open_input: finish::OpenInput,
    /// The target's vertices before filleting: those after it that are not among them are the
    /// fillet's (on its faces, or where it split the target's edges), placed on their faces'
    /// corners after the merge (`boolean::merge::p1::place_vertices_on_face_corners`).
    vertices_before: BTreeSet<VertexId>,
    extended_surfaces: BTreeMap<FaceId, Surface>,
    edge_caches: BTreeMap<EdgeId, CurveCache<FilletObject>>,
    target_serial: u64,
    self_intersection_handler: Option<MakeSurfaceBlendSih>,
    curve_blend_self_intersection_handler: Option<MakeCurveBlendSih>,
    /// The fork's `m_bSweptTurns` (`SetSweptTurns`; WR-697): a blend corner's Turn is
    /// Swept, not Mitred.
    swept_turns: bool,
    /// Not upstream (#10876): note the blend corner's Roll-over corners the corners step
    /// refuses ([`FilletSession::set_roll_over_classify`]).
    roll_over_classify: bool,
    /// Not upstream (#10876): the Roll-over corners noted.
    roll_overs: Vec<crate::blend::blend_corner::rollover::RollOver>,
    /// Not upstream (#10876): the session is the Roll-over's second pass
    /// ([`FilletSession::set_roll_over_pass`]), and its vertices V, the corners where the
    /// concave chain meets the cliff (empty: not the second pass).
    roll_over_ends: Vec<VertexId>,
    /// The fork's `m_vSetBackVertices` (`AddSetBackVertex`; WR-697, 79d4763): the
    /// vertices the caller sets back, where no Ball is built.
    set_back_vertices: Vec<VertexId>,
    /// The fork's `m_vSetBackFills` (`SetSetBackFill`; smlib#104 61547af): the corners
    /// asked to be set back, before the corners are made.
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
    /// The corner type of chamfers meeting at a vertex (`set_chamfer_corner`; WR-716),
    /// `None` where the caller asked none.
    chamfer_corner: Option<crate::blend::blend_corner::BlendChamferCorner>,
    band_cut_cache: Option<band_cuts::BandCutCache>,
    /// (not upstream) The edges `prepare_chamfers_along` chamfered as knife edges: equal
    /// distances, the chord between the contacts of the ball of that radius, kept as the
    /// linear section on offsets equal to the distance. Their solvers keep no chamfer
    /// definition (there is no wedge), so [`FilletSession::chamfer_fit`] reads them here.
    knife_chamfers: Vec<EdgeId>,
}

impl SessionObjects<'_> {
    /// The edges of the session's blends other than solver `id`'s: where a chain runs on into
    /// the next round.
    fn other_blends_edges(&mut self, id: FilletSolverId) -> Result<Vec<EdgeId>> {
        let mut joined = Vec::new();
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
        Ok(joined)
    }

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

    /// An additional parallel-line overlap certificate, including when current
    /// rails have passed completely beyond the opposite edges. A missing
    /// certificate leaves the independent rail checks to decide their geometry.
    fn planar_bands_overlap(
        &mut self,
        k: &Kernel,
        pair: [FilletObject; 2],
        face: FaceId,
        blended: &[EdgeId],
        solvers: &[FilletObject],
    ) -> Result<bool> {
        let [FilletObject::Solver(ia), FilletObject::Solver(ib)] = pair else {
            return Err(invalid("object is not a solver"));
        };
        let contact = {
            let (a, b) = (self.solver(ia)?, self.solver(ib)?);
            let rail_on_face = |solver: &SolverObject| -> Result<Option<usize>> {
                for rail in 0..2 {
                    if let Some(use_) = solver.base().edgeuse(rail)? {
                        if crate::topology::edgeuse::p1::face(self.target, use_)? == Some(face) {
                            return Ok(Some(rail));
                        }
                    }
                }
                Ok(None)
            };
            let (Some(rail_a), Some(rail_b)) = (rail_on_face(a)?, rail_on_face(b)?) else {
                return Ok(false);
            };
            if !planar_bands::parallel_overlap_candidate(self.target, a, b, rail_a, rail_b)? {
                return Ok(false);
            }
            let mut corners = Vec::new();
            for &object in solvers {
                let FilletObject::Solver(id) = object else {
                    return Err(invalid("object is not a solver"));
                };
                if id == ia || id == ib {
                    continue;
                }
                if let Some(corner) =
                    planar_bands::corner_band(self.target, self.solver(id)?, face)?
                {
                    corners.push(corner);
                }
            }
            planar_bands::retained_contact(
                k,
                self.target,
                a,
                b,
                [rail_a, rail_b],
                blended,
                &corners,
                true,
            )?
        };
        let Some(contact) = contact else {
            return Ok(false);
        };
        if sample_off_face(k, self.target, contact.face, contact.at)? {
            return Ok(false);
        }
        Ok(contact.overlapping)
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
            return Err(null_defect(
                "SmBlendCorner::Classify",
                SOLVER_OF_BLENDED_EDGEUSE,
            ));
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
    fn constant_offsets(&self, id: FilletSolverId) -> Result<Option<[f64; 2]>> {
        self.solvers
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .ok_or_else(|| invalid("swept turn solver is not live"))?
            .constant_offsets()
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
        None => SolidError::blend(operation, crate::error::blend::BlendFailure::CornerRefused),
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

/// Not the fork's (SolidCraft #19454): the vertices, the edges or a face of the blend of
/// corner `failed` failed with `error`. Where that corner is an End on its end face whose
/// contact line passes over that face
/// ([`crate::blend::blend_corner::end::largest_ball_ending_on_end_face`]: the largest ball
/// whose rail still ends on the end face is smaller than the round), the round is refused as
/// too large: `SM_FILERR_NO_ROOM` noted at the vertex, the fit on the blended edge (that
/// largest ball less the fork's `FitMargin`, a thousand of the edge's tolerances, as the
/// other open fits: at that radius the rail's end only touches the end face's surface), and
/// the returned error the typed [`crate::error::blend::BlendFailure::NoRoom`]
/// ([`super::NoRoomReason::EndPassesOverEndFace`]) carrying that fit. Anything else returns
/// `error` as it is. Asked only after the failure, so it never refuses an End that builds.
/// A round within the margin of that largest ball is refused with the larger ones: its End
/// takes all of its blend, and its edges or its face fail. With `missed` (the vertices'
/// failure) only a rail is judged whose end the vertices did not find on the end face. The
/// refusal keeps `error`'s operation, the step that failed. A judgement that itself fails
/// establishes no fit, and `error` stands.
///
/// The fit named is the least over every such End of the session (the other corners are
/// judged the same way, on their topology, whether or not their vertices are computed yet),
/// at its vertex and on its edge: a size under one End's limit and over another's builds
/// neither.
///
/// # Errors
/// Those of the judgement and of the topology queries.
#[allow(clippy::too_many_arguments)] // the session's borrowed object stores
fn end_passing_over_end_face(
    k: &Kernel,
    target: &mut Brep,
    pseudo: &FilletBrep,
    solvers: &[Option<SolverObject>],
    extended_surfaces: &BTreeMap<FaceId, Surface>,
    corners: &[FilletCorner],
    failed: usize,
    missed: bool,
    executive: &mut FilletExecutive<FilletObject>,
    error: SolidError,
) -> Result<SolidError> {
    use crate::blend::blend_corner::end::{
        describe_end_passing_over_end_face, largest_ball_ending_on_end_face,
    };
    use crate::blend::fillet_executive::NoRoomReason;
    // the refusal keeps the operation of the failure it stands for: the step that failed
    let operation = error.operation();
    let lookup = SweptSessionSolvers {
        solvers,
        executive: None,
    };
    // the fit of one corner, with its blended edge: `None` where it is not such an End.
    // `missed`: only of a rail whose end the corner's vertices did not find on the end face
    let judge = |target: &mut Brep,
                 corner: &FilletCorner,
                 missed: bool|
     -> Result<Option<(f64, Option<EdgeId>)>> {
        let Some(first) = corner
            .solver_eus
            .as_deref()
            .and_then(|eus| eus.first().copied())
        else {
            return Ok(None);
        };
        let edge = crate::topology::edgeuse::p1::edge(target, first)?;
        // FitMargin: a thousand of the blended edge's tolerances
        let margin = match edge {
            Some(e) => 1000.0 * crate::topology::edge::tolerance(target, e)?,
            None => 0.0,
        };
        let largest = largest_ball_ending_on_end_face(
            k,
            target,
            pseudo,
            corner,
            &lookup,
            extended_surfaces,
            margin,
            missed,
        )?;
        Ok(largest.map(|largest| (largest - margin, edge)))
    };
    let Some(this) = corners.get(failed) else {
        return Ok(error);
    };
    // a judgement that fails establishes no fit: the construction's own failure stands
    let mut least = match judge(target, this, missed) {
        Ok(Some(found)) => found,
        Err(e) if e.is_unported_or_cancelled() => return Err(e),
        Ok(None) | Err(_) => return Ok(error),
    };
    let mut at = this;
    for (i, other) in corners.iter().enumerate() {
        if i == failed {
            continue;
        }
        // another corner's judgement that fails proves nothing of this one's
        let judged = match judge(target, other, false) {
            Ok(judged) => judged,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => None,
        };
        if let Some(found) = judged {
            if found.0 < least.0 {
                least = found;
                at = other;
            }
        }
    }
    let (fit, edge) = least;
    if fit <= 0.0 {
        return Ok(error);
    }
    let vertex = at.filleted_vertex();
    let location = match vertex {
        Some(v) => crate::topology::vertex::point(target, v).ok(),
        None => None,
    };
    executive.error.note_vertex_error(
        vertex.map(|v| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))),
        location,
        crate::blend::fillet_geom::p1::FilStatus::Failure,
        crate::blend::fillet_geom::p1::FilletErrorType::NoRoom,
        Some(&describe_end_passing_over_end_face(fit)),
    );
    executive.error.no_room = Some(NoRoomReason::EndPassesOverEndFace);
    let edge_object = edge.map(|e| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e)));
    executive.note_fit_radius(
        fit,
        edge_object,
        crate::blend::fillet_executive::p3::FitKind::Radius,
    );
    let mut failure = crate::error::Failure::new(
        operation,
        crate::error::blend::BlendFailure::NoRoom(NoRoomReason::EndPassesOverEndFace),
    )
    .with_fit(crate::error::Fit::new(
        fit,
        crate::error::FitMeasure::Radius,
        edge_object
            .as_ref()
            .and_then(crate::error::TargetEdge::of_fillet_object),
    ));
    if let Some(v) = vertex {
        failure = failure.at(EntityRef::Vertex(v));
    }
    Ok(failure.into_error())
}

/// `SmFilletCorner::CalcCornerVertGeom` of the ordinary Nx1, Nx2 (the blend corners, the
/// closed Turn among them) and `NxN`
/// corner `id` over the live session's objects (the corner sits in its slot of
/// `corners`). Not upstream (#2406): at an End that can wait
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
            .ok_or_else(|| null_defect(OP, CORNER_OF_ITS_ID))
    }
}

fn live_vertex_solvers<'s>(
    pseudo: &FilletBrep,
    solvers: &'s mut [Option<SolverObject>],
    fv: VertexId,
) -> Result<Vec<(FilletSolverId, &'s mut dyn FilletSolverOps)>> {
    let mut ids = Vec::new();
    for geometry in pseudo.vertex_fillet_geoms(fv)? {
        let id = pseudo.geom(geometry)?.solver.ok_or_else(|| {
            SolidError::blend_solver(
                OP,
                crate::error::blend::SolverFailure::FilletGeomWithoutSolver {
                    fillet_geom: geometry,
                },
            )
        })?;
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
    /// The executive's `m_vExtendedSurfacesMap`.
    extended_surfaces: &'a mut BTreeMap<FaceId, Surface>,
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
            .ok_or_else(|| null_defect(OP, CORNER_OF_ITS_ID))
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
            return Err(SolidError::blend(
                OP,
                crate::error::blend::BlendFailure::FilletVertexWithoutCorner,
            ));
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
    // (not upstream) the note's own site, written below, or none of it
    error.no_room_without_site = note.no_room_without_site;
    // (not upstream) a solver's note is not explained by an earlier corner's classification
    error.corner_cause = None;
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
    error.no_room_without_site = false; // (not upstream: it names the vertex)
    error.corner_cause = None; // (not upstream: nor is it an earlier classification's)
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
                open_input: finish::OpenInput::default(),
                vertices_before: BTreeSet::new(),
                extended_surfaces: BTreeMap::new(),
                edge_caches: BTreeMap::new(),
                target_serial,
                self_intersection_handler: None,
                curve_blend_self_intersection_handler: None,
                swept_turns: false,
                roll_over_classify: false,
                roll_overs: Vec::new(),
                roll_over_ends: Vec::new(),
                set_back_vertices: Vec::new(),
                set_back_fills: Vec::new(),
                radius_corner_surfaces: Vec::new(),
                setback_refusal: None,
                blend_face_marks: Vec::new(),
                chamfer_corner: None,
                band_cut_cache: None,
                knife_chamfers: Vec::new(),
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
        crate::profile::profile_scope!(k, "blend::session::prepare");
        if edges.is_empty() {
            return Err(SolidError::blend(
                OP,
                crate::error::blend::BlendFailure::NoEdgesGiven,
            ));
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
    /// split (WR-665) applies to it.
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
        crate::profile::profile_scope!(k, "blend::session::prepare");
        if edges.is_empty() {
            return Err(SolidError::blend(
                OP,
                crate::error::blend::BlendFailure::NoEdgesGiven,
            ));
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
            return Err(SolidError::blend(
                OP,
                crate::error::blend::BlendFailure::NoEdgesGiven,
            ));
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
    /// thumbweight is `SetFilletParameters`' default with the G^n section, the fork's
    /// `SM_GN_DEFAULT_THUMBWEIGHT` (upstream's `1.0`, which the G^n section refuses).
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
        crate::profile::profile_scope!(k, "blend::session::prepare");
        if edges.is_empty() {
            return Err(SolidError::blend(
                OP,
                crate::error::blend::BlendFailure::NoEdgesGiven,
            ));
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
                // SetFilletParameters' default thumbweight: the fork's
                // SM_GN_DEFAULT_THUMBWEIGHT with its G^n section (WR-685), which refuses
                // upstream's 1.0
                thumbweight: crate::blend::fillet_section::section::GN_DEFAULT_THUMBWEIGHT,
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
        crate::profile::profile_scope!(k, "blend::session::create_corners");
        // solvers loaded since the last sync (`SetFilletParameters`) read the handler
        // during the corners' traces
        self.sync_self_intersection_handler();
        // (not upstream) a round rolls over its faces' smooth far edges, the whole length
        self.roll_over_tangent_edges(k)?;
        let SessionObjects {
            target,
            solvers,
            corners,
            swept_turns,
            set_back_vertices,
            chamfer_corner,
            set_back_fills,
            roll_over_classify,
            roll_overs,
            roll_over_ends,
            ..
        } = &mut self.objects;
        let swept_turns = *swept_turns;
        let roll_over_classify = *roll_over_classify;
        let roll_over_ends = &*roll_over_ends;
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
                let made = FilletCorner::create_noting_roll_overs(
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
                            return false;
                        };
                        solvers
                            .get(id.0 as usize)
                            .and_then(Option::as_ref)
                            .is_some_and(|s| {
                                // only offsets fixed along the edge are a constant reach
                                // (`OffsetRadiiCanChange`, as the swept corners read it): a
                                // constant-distance solver's offsets are solved per section
                                let can_change = match s {
                                    SolverObject::ConstantRadius(s) => {
                                        FilletSolverOps::offset_radii_can_change(&**s)
                                    }
                                    SolverObject::VariableRadius(s) => {
                                        FilletSolverOps::offset_radii_can_change(&**s)
                                    }
                                    SolverObject::ConstantDistance(s) => {
                                        FilletSolverOps::offset_radii_can_change(&**s)
                                    }
                                    SolverObject::SurfaceSurface(s) => {
                                        FilletSolverOps::offset_radii_can_change(&**s)
                                    }
                                };
                                matches!(can_change, Ok(false))
                            })
                    },
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
                    roll_over_classify.then_some(&mut *roll_overs),
                )?;
                let Some(mut corner) = made else {
                    // The vertex's point feeds the opt-in vertex error note, as the
                    // fork's corrected overload reads it from the vertex itself.
                    let location = crate::topology::vertex::point(target, vertex).ok();
                    return Ok(CornerCreation::Unsupported {
                        location,
                        cause: None,
                    });
                };
                // the Roll-over's second pass (#10876): its corner at V, where the concave
                // chain meets the cliff, looks for rail ends across the cliff's tangent seam;
                // its other corners are made as in any pass
                corner.roll_over = roll_over_ends.contains(&vertex);
                // Not the fork's (WR-716, #2345): chamfers meeting at a vertex where every
                // edge is chamfered are the blend corner's chamfer event, closed by their
                // planes, of the session's corner type; refused where that type does not
                // exist there (with no type asked, the type that exists: see
                // `classify_chamfer`); a corner the event declines (an edge not straight, a face
                // not a plane, a chamfer not of constant width) stays the pinned 3x3 one
                if corner.chamfer && corner.kind == FilletCornerKind::ConvexNByN {
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
                        ChamferClass::MixedConvexity => {
                            corner.chamfer_mixed_convexity = true;
                        }
                        ChamferClass::Declined(_) => {}
                    }
                }
                // The fork's (WR-697, 79d4763): three equal rounds of one convexity, every
                // edge there blended, are the blend corner's Ball
                // (SmBlendCorner::ClassifyBall, in SmFilletCorner::Create's three-solver
                // case before the 3x3 corner); a failed classification builds no corner and
                // its error stays with the note of the vertex.
                // Since smlib#125 (278bf77) four or more rounds that one ball touches are
                // the Ball too (ClassifyBall in the NxN case, before ClassifySetBack, where
                // every edge use is a solver's). A chamfer corner's rounds are not
                // circular, so ClassifyBall finds no Ball there before any test that could
                // fail.
                if matches!(
                    corner.kind,
                    FilletCornerKind::ConvexNByN | FilletCornerKind::ConcaveNByN
                ) && corner.all_eus.as_ref().map_or(0, Vec::len) >= 3
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
                        Err(cause) => return unclassified_corner(target, vertex, cause),
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
                // after the Ball); a failed classification builds no corner and its error
                // stays with the note of the vertex. The others as before.
                if corner.blend.is_none()
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
                        Err(cause) => return unclassified_corner(target, vertex, cause),
                        Ok(class)
                            if class.event
                                == crate::blend::blend_corner::BlendCornerEvent::Setback =>
                        {
                            corner = crate::blend::blend_corner::setback::setback_corner_of(
                                &corner,
                                class.order,
                            );
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
                if crate::blend::blend_corner::setback::set_back_fill_of(
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
                    swept_turns && blend.event == crate::blend::blend_corner::BlendCornerEvent::Turn
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

    /// Build one corner's pseudo-topology by its class (`SmFilletCorner::MakeCornerTopology`,
    /// virtual): the dispatch the executive's corner-vertex loop runs per corner, so an
    /// `NByOne` corner builds as [`FilletSession::make_nx1_corner_topology`] and a blend
    /// corner (the closed Turn among them, #2349) by its event.
    ///
    /// # Errors
    /// Unresolved corner/solver identities or the corner's topology producer's errors.
    pub fn make_corner_topology(&mut self, k: &mut Kernel, id: FilletCornerId) -> Result<()> {
        self.objects
            .vertices_make_topology(k, FilletObject::Corner(id), &mut self.executive)
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
        let res = self
            .executive
            .create_fillet_brep(k, &mut self.objects, error_info);
        self.objects.band_cut_cache = None;
        res
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

    /// Where the executive noted its last failure, on the caller's body, as the typed failure
    /// record names it ([`crate::error::Site`]): the error info's filleted edge, side face (else
    /// end face) and filleted vertex where each is topology of the target body (an object of
    /// an intermediate body names nothing), and its point. A no-room refusal noted without a
    /// site (`ErrorInfo::no_room_without_site`) names none: the record's site fields are
    /// then an earlier failure's. Reading it changes nothing.
    #[must_use]
    pub fn error_site(&self) -> crate::error::Site {
        use crate::error::{Site, TargetEdge, TargetFace, TargetVertex};
        let info = &self.executive.error;
        // a no-room refusal noted without a site: what the record holds is an earlier
        // failure's
        if info.no_room_without_site
            && info.code == crate::blend::fillet_geom::p1::FilletErrorType::NoRoom as i32
        {
            return Site::default();
        }
        Site {
            edge: info.edge.as_ref().and_then(TargetEdge::of_fillet_object),
            face: info
                .side_face
                .as_ref()
                .and_then(TargetFace::of_fillet_object)
                .or_else(|| {
                    info.end_face
                        .as_ref()
                        .and_then(TargetFace::of_fillet_object)
                }),
            vertex: info
                .vertex
                .as_ref()
                .and_then(TargetVertex::of_fillet_object),
            at: info.location,
        }
    }

    /// The fork's `SetSweptTurns` (smlib#79 step 4b; WR-697): a blend corner's Turn (two rounds of one
    /// convexity that turn at a sharp side edge) is Swept, its corner face the rounds' end
    /// section revolved about the side edge (an exact torus), instead of Mitred; a Swept
    /// turn the blend corner does not build is refused with its reason. Set it before
    /// [`Self::do_fillet`] (the corners are made there).
    pub fn set_swept_turns(&mut self, swept: bool) {
        self.objects.swept_turns = swept;
    }

    /// Not upstream (#10876): note the blend corner's Roll-over corners
    /// ([`crate::blend::blend_corner::rollover::classify_roll_over`]) that the corners step
    /// refuses as a convex and a concave round meeting at four edges; read them with
    /// [`Self::roll_overs`]. The refusal is unchanged: the caller builds the event in two
    /// passes. Off by default. Set it before [`Self::create_corners`].
    pub fn set_roll_over_classify(&mut self, classify: bool) {
        self.objects.roll_over_classify = classify;
    }

    /// Not upstream (#10876): the Roll-over corners noted by the corners step
    /// ([`Self::set_roll_over_classify`]), in vertex order.
    #[must_use]
    pub fn roll_overs(&self) -> &[crate::blend::blend_corner::rollover::RollOver] {
        &self.objects.roll_overs
    }

    /// Not upstream (#10876): the session is the blend corner's Roll-over's second pass, the
    /// concave chain (B2's remainder with B1's end arc on its end face), handed the first
    /// pass's faces, with `ends` its vertices V where the chain meets the cliff
    /// ([`crate::blend::blend_corner::rollover::concave_chain_ends`]). Its blends' concave
    /// overflow across the cliff is accepted, built as a cliff overflow and trimmed at the
    /// creases (with `fixes.fillet_cliff_overflow` and
    /// `fixes.fillet_refuse_rails_leaving_faces`), and its corners at the vertices of `ends`
    /// (only those) look for a rail's end across the cliff's tangent seam. Empty `ends`: not
    /// the second pass (the default). Set it before [`Self::create_corners`].
    pub fn set_roll_over_pass(&mut self, ends: Vec<VertexId>) {
        self.executive.set_roll_over_pass(!ends.is_empty());
        self.objects.roll_over_ends = ends;
    }

    /// How chamfers meeting at a vertex where every edge is chamfered close the corner
    /// (the blend corner's chamfer event, WR-716): the Mitre (the chamfer planes run on to
    /// meet each other) or the Facet (one plane through the rails' crossings), refused
    /// where the asked type does not exist. Unset, the corner is the Mitre where it exists,
    /// at four or more chamfers the patch or (crossings in one plane) the Facet, and
    /// otherwise the pinned corner, never a refusal of the type (`classify_chamfer`). It
    /// overrides the corner's bevel flag ([`Self::set_corner_bevel`]) at
    /// such a corner. Set it before [`Self::do_fillet`] (the corners are made there).
    pub fn set_chamfer_corner(&mut self, corner: crate::blend::blend_corner::BlendChamferCorner) {
        self.objects.chamfer_corner = Some(corner);
    }

    /// The fork's `AddSetBackVertex` (smlib#79 step 4c; WR-697): the caller sets
    /// `vertex` back (its corner's setback distance, set after the corners are made), so
    /// no Ball is built there (`IsSetBackVertex`, read by `SmBlendCorner::ClassifyBall`).
    /// Between planes, with a setback set on its corner ([`Self::set_corner_setback`], its
    /// ease with [`Self::set_corner_ease`]), the corner is then the blend corner's Setback,
    /// decided when the corners are filleted and filled as one asked with
    /// [`Self::set_set_back_fill`]; without one it stays the N x N corner (not the fork,
    /// solidcore#2933).
    /// Add it before [`Self::do_fillet`].
    pub fn add_set_back_vertex(&mut self, vertex: VertexId) {
        self.objects.set_back_vertices.push(vertex);
    }

    /// The fork's `SetSetBackFill` (smlib#104 61547af): the corner at `vertex` set back
    /// `setback` along each of its edges, with an ease `ease` before it (0: none): asked
    /// before [`Self::do_fillet`], which then makes that corner the blend corner's Setback
    /// (the vertex is no Ball, as one marked with [`Self::add_set_back_vertex`] is not),
    /// filled by the setback split, where it can.
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

    /// Not the fork (solidcore#2933): each N x N corner at a vertex marked set back
    /// ([`Self::add_set_back_vertex`]) with a setback set on it ([`Self::set_corner_setback`],
    /// its ease with [`Self::set_corner_ease`]) is classified where its setback is first read,
    /// before any corner's geometry is made: by `classify_setback` with that setback asked as
    /// its vertex's fill. The setback is not known when [`Self::create_corners`] classifies the
    /// corners: it is set on a made corner, by its identity (the fork's `SetSetBackDist`), and
    /// every caller sets it after the corners are made: `solidcore-ops`' `blend::pass` makes
    /// them and `blend::corners` sets it, SolidCraft's `smlib::core::blend` likewise
    /// (`blend_edges_stopping`, then `corners`), and the goldens runner's
    /// `setback_fillet_edges`. Where the corner is the Setback it takes the N x N
    /// corner's place, with its fill asked as [`Self::set_set_back_fill`] asks one; elsewhere
    /// (a corner `classify_setback` does not take, no setback set) it stays the N x N corner.
    ///
    /// Likewise a corner that is the Setback already (rounds of unequal radii, or
    /// curvature-continuous sections, which [`Self::create_corners`] classifies without being
    /// asked), marked or not, with a setback set on it: that setback, with its ease, is asked
    /// as its vertex's fill, so the corner builds it (or refuses it, naming one that builds:
    /// `make_setback_topology`) instead of the rule's setbacks, which read no setback set on
    /// the made corner (#16408). It is classified again with the fill asked, as the fill's
    /// order follows the ease. The one Setback that cannot take the ask is one beside a curved
    /// face asked an ease (its fill is G1 only, `classify_setback`): that corner is returned,
    /// for the session to refuse rather than build the rule's setbacks without the ask.
    ///
    /// # Errors
    /// Those of the classification.
    pub(super) fn set_back_marked_corners(&mut self, k: &Kernel) -> Result<Option<VertexId>> {
        use crate::blend::blend_corner::setback::{
            classify_setback, set_back_fill_of, setback_corner_of, SetBackFill, SetbackCorner,
        };
        let SessionObjects {
            target,
            solvers,
            corners,
            set_back_vertices,
            set_back_fills,
            ..
        } = &mut self.objects;
        let lookup = SweptSessionSolvers {
            solvers: solvers.as_slice(),
            executive: Some(&self.executive),
        };
        for corner in corners.iter_mut() {
            let Some(vertex) = corner.vertex else {
                continue;
            };
            let setback = corner.set_back_dist;
            // the Setback already (#16408), or an N x N corner a mark keeps from the Ball
            let is_setback = corner
                .blend
                .is_some_and(crate::blend::blend_corner::BlendCorner::is_setback);
            let marked_nxn = matches!(
                corner.kind,
                FilletCornerKind::ConvexNByN | FilletCornerKind::ConcaveNByN
            ) && corner.blend.is_none()
                && set_back_vertices.contains(&vertex);
            if !(is_setback || marked_nxn)
                || corner.chamfer
                || setback.is_nan()
                || setback <= 0.0
                || set_back_fill_of(set_back_fills, vertex).is_some()
            {
                continue;
            }
            let fill = SetBackFill {
                vertex,
                setback,
                ease: if corner.ease_dist > 0.0 {
                    corner.ease_dist
                } else {
                    0.0
                },
                open: false,
            };
            let mut asked = set_back_fills.clone();
            asked.push(fill);
            let class = classify_setback(
                k,
                target,
                vertex,
                corner.solver_eus.as_deref().unwrap_or_default(),
                corner.all_eus.as_deref().unwrap_or_default(),
                &lookup,
                crate::math::rad_to_deg(corner.tangency_tol_radians),
                &asked,
            )?;
            if class.event != crate::blend::blend_corner::BlendCornerEvent::Setback {
                if is_setback {
                    return Ok(Some(vertex));
                }
                continue;
            }
            if is_setback {
                // its fill's order with the ease asked (2 with one, as created without it)
                corner.setback = Some(SetbackCorner::with_order(class.order));
            } else {
                *corner = setback_corner_of(corner, class.order);
            }
            set_back_fills.push(fill);
        }
        Ok(None)
    }

    /// The fork's `GetFitSetBack` (smlib#104 f1d81f4): with a setback corner refused as too
    /// short for a convex fill (`FilletErrorType::NoRoom`), the least setback that builds
    /// one: `> 0`, `0` when none does, `< 0` when not that cause.
    #[must_use]
    pub fn fit_setback(&self) -> f64 {
        self.executive.fit_setback
    }

    /// `GetFitRadius`, `GetFitEdge`, `GetFitKind` (the fork's, tangency-labs/smlib#79
    /// a3d6d81): with a refusal as too large ([`ErrorInfo::no_room_reason`]), the largest
    /// size that builds the refused blend and its edge, computed from the refusal's cause;
    /// `None` when its cause has no size (a radius law) or no refusal noted one.
    #[must_use]
    pub fn fit_size(&self) -> Option<crate::blend::fillet_executive::FitSize<EdgeId>> {
        let fit = self.executive.fit_size()?;
        // the mapping to the caller's body the typed failure record's site uses
        let edge = fit
            .edge
            .as_ref()
            .and_then(crate::error::TargetEdge::of_fillet_object)
            .map(crate::error::TargetEdge::id);
        Some(crate::blend::fillet_executive::FitSize {
            size: fit.size,
            edge,
            kind: fit.kind,
        })
    }

    /// The chamfer's first distance (its setback along its first face) at which a chamfer
    /// refused as too large builds: the fit `fit` ([`Self::fit_size`], or a refusal's) read
    /// through the chamfer's definition on its edge ([`FilletSolver::chamfer`]). At the
    /// edge's wedge the offsets are linear in the sizes, `r_1 = (d_2 - d_1 cos a) / sin a`,
    /// `r_2 = (d_1 - d_2 cos a) / sin a` ([`ChamferDefinition`]), so the chamfer whose offsets
    /// are `lambda` times the asked ones is the asked chamfer scaled by `lambda`
    /// ([`ChamferDefinition::scaled`]): exact in the maps, the distance named to the rounding
    /// of the fit and of the offset and distance conversions. A fit of a radius
    /// (`FitKind::Radius`, an equal chamfer's one offset) is `lambda = fit / r_1`; a fit of a
    /// scale on two unequal offsets (`FitKind::Scale`) is `lambda` itself. A knife edge's
    /// chamfer (equal distances, the chord between the contacts of the ball of that radius)
    /// has offsets equal to its distance and no definition: a radius fit is its distance.
    /// `None` for a fit on no edge or a chord, an edge whose solver keeps no chamfer and is no
    /// knife edge chamfered, a scale fit of a knife edge, or a radius fit of a chamfer whose
    /// two offsets differ (the fit is then a scale).
    ///
    /// [`FilletSolver::chamfer`]: crate::blend::fillet_solver::p1::FilletSolver::chamfer
    /// [`ChamferDefinition`]: crate::surface::procedural_blend::chamfer::ChamferDefinition
    /// [`ChamferDefinition::scaled`]: crate::surface::procedural_blend::chamfer::ChamferDefinition::scaled
    #[must_use]
    pub fn chamfer_fit(&self, fit: crate::blend::fillet_executive::FitSize<EdgeId>) -> Option<f64> {
        use crate::blend::fillet_executive::p3::FitKind;
        let edge = fit.edge?;
        let target = self.target();
        let chamfer = self.objects.solvers.iter().flatten().find_map(|solver| {
            let base = solver.base();
            let eu = base.edgeuse(0).ok()??;
            (crate::topology::edgeuse::p1::edge(target, eu).ok()? == Some(edge))
                .then_some(base.chamfer)?
        });
        let Some(chamfer) = chamfer else {
            // a knife edge: its offsets are its distance, so the radius that fits is the
            // distance that fits
            let knife = self.objects.knife_chamfers.contains(&edge);
            return (knife
                && fit.kind == FitKind::Radius
                && fit.size.is_finite()
                && fit.size > 0.0)
                .then_some(fit.size);
        };
        let [r1, _] = chamfer.offsets().ok()?;
        let lambda = match fit.kind {
            // an equal chamfer's one offset: both offsets are r_1, to the conversion's rounding
            FitKind::Radius if chamfer.holds([r1, r1]) => fit.size / r1,
            FitKind::Scale => fit.size,
            FitKind::Radius | FitKind::Chord => return None,
        };
        let [d1, _] = chamfer.scaled(lambda).setbacks();
        (lambda.is_finite() && lambda > 0.0).then_some(d1)
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

    /// `SmFilletCorner::SetSetBackDist` on one live corner. A setback (`distance > 0`) set on
    /// a corner that builds none (neither the blend corner's Setback nor an N x N corner,
    /// `blend_corner::setback::builds_setback`) is refused by [`Self::do_filleting`]
    /// (`CornerUnsupported`) rather than built without it. On a corner that is the Setback
    /// already it is the corner's asked setback, as one asked with [`Self::set_set_back_fill`]
    /// is (`set_back_marked_corners`).
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
        _k: &mut Kernel,
        corner: FilletObject,
        _executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        // AdjustTangentCorner is the base class's SM_SUCCESS at every corner: the Nx2
        // override (AdjustTwoBlendTangentCorner) went with the fork's smlib#113 (34288e5),
        // the closed Nx1 corner's with caa67d0, and every two-blend corner is a blend
        // corner, whose events adjust nothing (#2400)
        self.corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        Ok(())
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
                .ok_or_else(|| null_defect(OP_NOTE_OPEN_END, SOLVER_OF_BLENDED_EDGEUSE))?;
            open_ends.push(OpenEnd {
                solver,
                vertex: FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(vertex)),
                end_face: FilletObject::Topology(FilletBody::Target, EntityRef::Face(end_face)),
                section,
            });
            Ok(())
        };
        if corner
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
                // in the adjusted corner's pass the rollover has run: an End that would wait
                // again would be left without edges, its cap built on none (#15906). A stage
                // failure with no reason the kernel names (a defect), built on purpose
                if executive.rollover_adjusting {
                    return Err(defect(
                        "SmBlendCorner::CalcEndEdgeGeom",
                        "in the adjusted corner's pass the rollover has run, so no End waits for it again",
                    ));
                }
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
                return Err(SolidError::blend(
                    "SmBlendCorner::CalcEndEdgeGeom",
                    crate::error::blend::BlendFailure::CornerRefused,
                ));
            }
        } else {
            // WR-674: a round's end where its edge runs on tangentially is left open too.
            let dropped =
                corner.drop_end_rib(k, target, pseudo, cliff_overflow, true, &mut note_open_end)?;
            executive.open_ends.append(&mut open_ends);
            if dropped == EndRibDrop::GlobalMerge {
                executive.options.global_merge = true;
            }
        }
        let mut context = SessionCornerEdgeContext {
            k: &mut *k,
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
        let built = corner.calc_corner_edge_geom_with(
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
        );
        match built {
            // an End whose contact line passes over its end face is refused as too large,
            // naming the radius that fits (noted, so the executive keeps it)
            Err(e) if !e.is_unported_or_cancelled() => Err(end_passing_over_end_face(
                k,
                target,
                pseudo,
                solvers.as_slice(),
                extended_surfaces,
                corners.as_slice(),
                id.0 as usize,
                false,
                executive,
                e,
            )?),
            built => built,
        }
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
    fn mapping_split_rail(
        &mut self,
        k: &mut Kernel,
        geom: FilletObject,
        rail: FilletObject,
        candidates: &[FilletObject],
    ) -> Result<()> {
        let FilletObject::Geometry(g) = geom else {
            return Err(invalid("mapping geometry is not retained"));
        };
        let FilletObject::Topology(FilletBody::Pseudo, EntityRef::Edge(rail)) = rail else {
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
        let record = self.pseudo.geom(g)?;
        let Some(side) = record.rails.iter().position(|r| *r == Some(rail)) else {
            return Err(invalid("mapping rail is not the geometry's"));
        };
        // a rail already split (a cliff's) keeps its pieces
        let others = if side == 0 {
            &record.other_rails1
        } else {
            &record.other_rails2
        };
        if !others.is_empty() {
            return Ok(());
        }
        let body = self
            .intermediate
            .as_ref()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        let Some(pieces) = self
            .pseudo
            .fillet_brep_pieces_along(k, rail, body, &candidates)?
        else {
            return Ok(());
        };
        let pieces = crate::blend::fillet_solver::p1::RetainedRailOwnership::with_context(
            k,
            &mut self.pseudo,
            &mut self.attributes,
            |k, pseudo, ownership| {
                crate::blend::fillet_solver::p1::split_rail_along(
                    k, pseudo, ownership, g, rail, &pieces,
                )
            },
        )?;
        let record = self.pseudo.geom_mut(g)?;
        if side == 0 {
            record.other_rails1.extend(pieces);
        } else {
            record.other_rails2.extend(pieces);
        }
        Ok(())
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
        crate::topology::brep::p3::seed_records_since(
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
        let edgeuse = current
            .base()
            .edgeuse(0)?
            .ok_or_else(|| null_defect(OP, SOLVER_ON_ITS_EDGEUSE))?;
        let edge = crate::topology::edgeuse::p1::edge(self.target, edgeuse)?
            .ok_or_else(|| edgeuse_without_edge(OP, edgeuse))?;
        // a sheet's edge bounds no material: there is no part for the band to cut (not the
        // fork's, whose `sm_BlendAddsMaterial` fails there, and the fillet with it)
        if !crate::blend::fillet_solver::p5::edge_bounds_material(self.target, edge)?
            || crate::blend::fillet_solver::p5::is_concave_edge(k, self.target, edge)?
        {
            return Ok(None);
        }
        let Some(cut) = band_cuts::band_cuts_the_part_cached(
            k,
            self.target,
            current,
            &mut self.band_cut_cache,
        )?
        else {
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
        let edgeuse = current
            .base()
            .edgeuse(0)?
            .ok_or_else(|| null_defect(OP, SOLVER_ON_ITS_EDGEUSE))?;
        let edge = crate::topology::edgeuse::p1::edge(self.target, edgeuse)?
            .ok_or_else(|| edgeuse_without_edge(OP, edgeuse))?;
        // a sheet's edge bounds no material: there is no part for the band to cut (not the
        // fork's, whose `sm_BlendAddsMaterial` fails there, and the fillet with it)
        if !crate::blend::fillet_solver::p5::edge_bounds_material(self.target, edge)?
            || crate::blend::fillet_solver::p5::is_concave_edge(k, self.target, edge)?
        {
            return Ok(None);
        }
        // judged against the part alone first: the fills below are built only for a blend
        // that would break through without them
        let Some(alone) = band_cuts::band_breaks_through_the_part_cached(
            k,
            self.target,
            current,
            Vec::new(),
            &mut self.band_cut_cache,
        )?
        else {
            return Ok(None);
        };
        // the material the concave rounds of the same blend add, which the section may run
        // through (the rig's 1668-fillet-both-sides-of-thin-wall-r7). Counted only where
        // the round adds its whole section along its whole edge:
        // - not in the Roll-over's second pass, whose concave overflow across the cliff is
        //   accepted and trimmed flush with the cliff (`choose_cliff_overflow`); anywhere
        //   else a concave round whose rail leaves its face is refused there
        //   (`ConcaveOverflow`), so a fillet that builds has every rail on its face;
        // - not where an end of its edge is a set-back vertex, by either door (marked, or
        //   asked a fill), or meets the edge of another blend of this fillet: a corner or
        //   set-back patch replaces the round there, and its shape is not known yet (the
        //   corners are made after this judgement);
        // - and only where both ends of its edge are closed square (`ConcaveFill::of_round`)
        let mut fills = Vec::new();
        if self.roll_over_ends.is_empty() {
            let mut edges = Vec::new();
            for (index, solver) in self.solvers.iter().enumerate() {
                // only a solver the executive loaded (`LoadFilletSolver` binds it to the
                // executive): one allocated but never loaded is never built, so its round
                // adds nothing
                let Some(solver) = solver
                    .as_ref()
                    .filter(|solver| solver.base().fillet_executive().is_some())
                else {
                    continue;
                };
                if let Some(edge) = solver
                    .base()
                    .edgeuse(0)?
                    .map(|edgeuse| crate::topology::edgeuse::p1::edge(self.target, edgeuse))
                    .transpose()?
                    .flatten()
                {
                    edges.push((index, edge));
                }
            }
            let set_back = |v: VertexId| {
                self.set_back_vertices.contains(&v)
                    || crate::blend::blend_corner::setback::set_back_fill_of(
                        &self.set_back_fills,
                        v,
                    )
                    .is_some()
            };
            let mut candidates = Vec::new();
            for &(index, other_edge) in &edges {
                if index == id.0 as usize
                    || !crate::blend::fillet_solver::p5::is_concave_edge(
                        k,
                        self.target,
                        other_edge,
                    )?
                {
                    continue;
                }
                let (start, end) =
                    crate::topology::edge::start_end_vertices(self.target, other_edge)?;
                let (Some(start), Some(end)) = (start, end) else {
                    continue;
                };
                let mut alone = !set_back(start) && !set_back(end);
                for &(j, edge) in &edges {
                    if !alone || j == index {
                        continue;
                    }
                    let (s, e) = crate::topology::edge::start_end_vertices(self.target, edge)?;
                    alone = ![s, e].iter().flatten().any(|v| *v == start || *v == end);
                }
                if alone {
                    candidates.push(index);
                }
            }
            for index in candidates {
                let Some(other) = self.solvers.get_mut(index).and_then(Option::as_mut) else {
                    continue;
                };
                // a failure to solve the round's section counts no fill (the refusing side)
                if let Some(fill) = crate::error::native_failure(band_cuts::ConcaveFill::of_round(
                    k,
                    self.target,
                    other,
                ))?
                .flatten()
                {
                    fills.push(fill);
                }
            }
        }
        let current = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let cut = if fills.is_empty() {
            alone
        } else {
            match band_cuts::band_breaks_through_the_part_cached(
                k,
                self.target,
                current,
                fills,
                &mut self.band_cut_cache,
            )? {
                Some(cut) => cut,
                None => return Ok(None),
            }
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

    fn creation_corner_leaves_face(
        &mut self,
        k: &mut Kernel,
        corner: FilletObject,
        a: FilletObject,
        b: FilletObject,
    ) -> Result<Option<f64>> {
        let (FilletObject::Corner(c), FilletObject::Solver(ia), FilletObject::Solver(ib)) =
            (corner, a, b)
        else {
            return Err(invalid("the corner's fit takes a corner and two solvers"));
        };
        let vertex = self
            .corners
            .get(c.0 as usize)
            .and_then(|c| c.vertex)
            .ok_or_else(|| invalid("corner without its vertex"))?;
        let (ia, ib) = (ia.0 as usize, ib.0 as usize);
        if ia == ib {
            return Ok(None);
        }
        let (lo, hi) = (ia.min(ib), ia.max(ib));
        let (left, right) = self
            .solvers
            .split_at_mut_checked(hi)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let low = left
            .get_mut(lo)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let high = right
            .get_mut(0)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let (sa, sb) = if ia < ib { (low, high) } else { (high, low) };
        fit_size::corner_leaves_face(k, &*self.target, sa, sb, vertex)
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
        // triangle)
        let joined = self.other_blends_edges(id)?;
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

    fn creation_ball_past_an_end(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        radius: Option<f64>,
    ) -> Result<Option<crate::blend::fillet_exact::PastAnEnd>> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        // the other blends' edges, where a chain runs on into the next round
        let joined = self.other_blends_edges(id)?;
        let current = self
            .solvers
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("solver is not live in this session"))?;
        let (rolls, law) = ball_regular::rolls_a_ball(current);
        if !rolls || law {
            return Ok(None);
        }
        crate::blend::fillet_exact::ball_past_an_end(
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
    ) -> Result<Vec<BandFit<FilletObject>>> {
        use crate::blend::fillet_executive::FitKind;
        use crate::topology::edgeuse::p1 as eu;
        let mut blended = Vec::new();
        let mut chamfers = true;
        let mut common_ball = true;
        let mut radius: Option<f64> = None;
        for &object in solvers {
            let FilletObject::Solver(id) = object else {
                return Err(invalid("object is not a solver"));
            };
            let solver = self.solver(id)?;
            chamfers &= solver.base().chamfer.is_some();
            match solver.constant_offsets()? {
                Some(offsets) => {
                    for offset in offsets {
                        let offset = offset.abs();
                        let own = *radius.get_or_insert(offset);
                        common_ball &= offset > 0.0
                            && (offset - own).abs() <= crate::math::EFF_ZERO * (1.0 + own);
                    }
                }
                None => common_ball = false,
            }
            if let Some(edgeuse) = solver.base().edgeuse(0)? {
                if let Some(edge) = eu::edge(self.target, edgeuse)? {
                    blended.push(edge);
                }
            }
        }
        let mut out = Vec::new();
        for &a in solvers {
            for &b in solvers {
                let (FilletObject::Solver(ia), FilletObject::Solver(ib)) = (a, b) else {
                    return Err(invalid("object is not a solver"));
                };
                if ia == ib {
                    continue;
                }
                let (Some(ea0), Some(eb0)) = (
                    self.solver(ia)?.base().edgeuse(0)?,
                    self.solver(ib)?.base().edgeuse(0)?,
                ) else {
                    continue;
                };
                let edge =
                    eu::edge(self.target, ea0)?.ok_or_else(|| edgeuse_without_edge(OP, ea0))?;
                let other_edge =
                    eu::edge(self.target, eb0)?.ok_or_else(|| edgeuse_without_edge(OP, eb0))?;
                let (a0, a1) = crate::topology::edge::start_end_vertices(self.target, edge)?;
                let (b0, b1) = crate::topology::edge::start_end_vertices(self.target, other_edge)?;
                if edge == other_edge
                    || [a0, a1]
                        .iter()
                        .flatten()
                        .any(|v| [b0, b1].contains(&Some(*v)))
                {
                    continue;
                }
                for ka in 0..2 {
                    for kb in 0..2 {
                        let fa = match self.solver(ia)?.base().edgeuse(ka)? {
                            Some(e) => eu::face(self.target, e)?,
                            None => continue,
                        };
                        let fb = match self.solver(ib)?.base().edgeuse(kb)? {
                            Some(e) => eu::face(self.target, e)?,
                            None => continue,
                        };
                        let Some(face) = fa.filter(|f| Some(*f) == fb) else {
                            continue;
                        };
                        let planar = planar_bands::supports(
                            self.target,
                            self.solver(ia)?,
                            self.solver(ib)?,
                            ka,
                            kb,
                        )?;
                        let mut corners = Vec::new();
                        if planar {
                            for &object in solvers {
                                let FilletObject::Solver(id) = object else {
                                    return Err(invalid("object is not a solver"));
                                };
                                if id == ia || id == ib {
                                    continue;
                                }
                                if let Some(corner) =
                                    planar_bands::corner_band(self.target, self.solver(id)?, face)?
                                {
                                    corners.push(corner);
                                }
                            }
                        }
                        let (Some(mut this), Some(mut other)) = (
                            self.solvers.get_mut(ia.0 as usize).and_then(Option::take),
                            self.solvers.get_mut(ib.0 as usize).and_then(Option::take),
                        ) else {
                            return Err(invalid("solver is not live in this session"));
                        };
                        // Select the construction from the support definitions. A planar
                        // contact query that fails never falls back to a sampled search.
                        let meet = (|| -> Result<Option<BandFit<FilletObject>>> {
                            if planar {
                                return Ok(planar_bands::contact(
                                    k,
                                    self.target,
                                    &this,
                                    &other,
                                    [ka, kb],
                                    &blended,
                                    &corners,
                                    chamfers && !common_ball,
                                )?
                                .map(|fit| BandFit {
                                    size: fit.size,
                                    kind: fit.kind,
                                    edge: FilletObject::Topology(
                                        FilletBody::Target,
                                        EntityRef::Edge(fit.edge),
                                    ),
                                    face: Some(FilletObject::Topology(
                                        FilletBody::Target,
                                        EntityRef::Face(fit.face),
                                    )),
                                    at: Some(fit.at),
                                }));
                            }
                            let meet = fit_size::bands_meet_at(
                                k,
                                self.target,
                                &mut this,
                                &mut other,
                                ka,
                                kb,
                            )?;
                            Ok(meet.map(|size| BandFit {
                                size,
                                kind: FitKind::Radius,
                                edge: FilletObject::Topology(
                                    FilletBody::Target,
                                    EntityRef::Edge(edge),
                                ),
                                face: Some(FilletObject::Topology(
                                    FilletBody::Target,
                                    EntityRef::Face(face),
                                )),
                                at: None,
                            }))
                        })();
                        if let Some(slot) = self.solvers.get_mut(ia.0 as usize) {
                            *slot = Some(this);
                        }
                        if let Some(slot) = self.solvers.get_mut(ib.0 as usize) {
                            *slot = Some(other);
                        }
                        if let Some(mut meet) = meet? {
                            // Every chamfer in one call scales together. Mixed wedges can
                            // have equal offsets on one pair and unequal ones on another;
                            // compare their common scales, never lengths against factors.
                            if chamfers && !common_ball && meet.kind == FitKind::Radius {
                                let offsets = self
                                    .solver(ia)?
                                    .constant_offsets()?
                                    .ok_or_else(|| invalid("a chamfer's constant offsets"))?;
                                let own = offsets
                                    .first()
                                    .copied()
                                    .map(f64::abs)
                                    .filter(|r| r.is_finite() && *r > 0.0)
                                    .ok_or_else(|| invalid("a chamfer's positive offset"))?;
                                meet.size /= own;
                                meet.kind = FitKind::Scale;
                            }
                            out.push(meet);
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}

/// One solver's rails for the band check: per rail, the face it rolls on, its curve and its
/// kept interval.
type SolverRails = Vec<(
    FaceId,
    crate::curve::curve::Curve,
    crate::geom::extent1d::Extent1d,
)>;

/// Each solver's rails, its filleted edge with its ends, and the largest solver tolerance:
/// what the band check (`BandsClearOtherRails`) reads.
#[allow(clippy::type_complexity)]
fn band_rails(
    objects: &SessionObjects<'_>,
    k: &Kernel,
    solvers: &[FilletObject],
) -> Result<(Vec<SolverRails>, Vec<(EdgeId, [Option<VertexId>; 2])>, f64)> {
    use crate::blend::fillet_geom::p1::FilletGeomType;
    use crate::geom::global_solver::SolverOperation;
    use crate::topology::edgeuse::p1 as eu;
    let this = objects;
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
        let base = this.solver(id)?.base();
        if base.approx_tol_3d > tol {
            tol = base.approx_tol_3d;
        }
        let mut own = Vec::new();
        for geom in base.fillet_geoms() {
            let geom = this.pseudo.geom(geom)?;
            if geom.fillet_geom_type() != FilletGeomType::Default {
                continue;
            }
            for ii in 0..2 {
                let rail = geom
                    .rail(ii)
                    .ok_or_else(|| invalid("geometry without rail"))?;
                let curve = crate::topology::edge::curve(this.pseudo.brep(), rail)?
                    .ok_or_else(|| invalid("rail without curve"))?
                    .clone();
                let Some(face) = this.pseudo.fillet_edge(rail)?.original_face() else {
                    continue;
                };
                // The rail runs on past its ends (the surfaces are not trimmed yet),
                // as far as a mitre at a corner needs and beyond: the band is the
                // rail's part between the vertices that end it, where the corners have
                // placed them, or, at an End at a smooth vertex, where its cap will (any
                // other end no corner has placed is the rail's own; a rail whose ends are
                // one vertex is closed and whole). The fork's tangency-labs/smlib@d548305
                // (WR-649).
                let ivl = curve.natural_interval();
                let pseudo = this.pseudo.brep();
                let prim = eu::primary_edgeuse(pseudo, rail)?
                    .ok_or_else(|| invalid("rail without primary edgeuse"))?;
                let mate =
                    eu::mate(pseudo, prim)?.ok_or_else(|| invalid("rail edgeuse without mate"))?;
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
                        let fv = this.pseudo.fillet_vertex(end)?;
                        if !fv.is_processed() {
                            // An End at a smooth vertex places its rails' ends on its cap
                            // only in the corners' second pass, after this check. Where the
                            // face a rail rolls on runs on past that vertex, its run-on would
                            // be taken for band: its end is the rail's crossing with the cap
                            // nearest the vertex, as that pass finds it.
                            let Some(id) = fv.fillet_corner() else {
                                continue;
                            };
                            let corner = CornerLookup::table(&this.corners).corner(id)?;
                            let Some(vertex) = corner.filleted_vertex() else {
                                continue;
                            };
                            let near = crate::topology::vertex::point(this.target, vertex)?;
                            // Not the fork's (#18823): a rail that ends where it crosses a
                            // side edge continued past the vertex (an End of mixed
                            // convexity, as where a window's top edge meets its side wall)
                            // has its end placed in that pass too: the rail's crossing
                            // with the side face's surface, the face beside the side edge
                            // that is not the rail's
                            let side_face = match (fv.fillet_vertex_type(), fv.point_class_object())
                            {
                                (
                                    FilletVertexType::RailXExtendedEdgeuse,
                                    Some(EntityRef::Edgeuse(side)),
                                ) => {
                                    let mut across = None;
                                    for u in [Some(side), eu::radial(this.target, side)?]
                                        .into_iter()
                                        .flatten()
                                    {
                                        if let Some(f) = eu::face(this.target, u)? {
                                            if f != face {
                                                across = Some(f);
                                            }
                                        }
                                    }
                                    match across {
                                        // its plane continued past the face (a crossing
                                        // with a curved side face is left to that pass)
                                        Some(f) => match crate::topology::face::p2::surface(
                                            this.target,
                                            f,
                                        )?
                                        .map(Surface::kind)
                                        {
                                            Some(crate::surface::surface::SurfaceKind::Plane(
                                                pl,
                                            )) => {
                                                let (origin, normal, _) = pl.is_planar();
                                                Some(Surface::from(
                                                    crate::surface::plane::Plane::from_normal(
                                                        origin, normal,
                                                    ),
                                                ))
                                            }
                                            _ => None,
                                        },
                                        None => None,
                                    }
                                }
                                _ => None,
                            };
                            let Some(cap) = corner.end_cap.as_ref().or(side_face.as_ref()) else {
                                continue;
                            };
                            if let Some((t, _)) =
                                rail_crossing_nearest(k, &curve, cap, near, base.approx_tol_3d)?
                            {
                                *t_end = t;
                            }
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
        let edge =
            eu::edge(this.target, fil_eu)?.ok_or_else(|| invalid("solver edgeuse without edge"))?;
        let ends = [
            crate::topology::edge::start_vertex(this.target, edge)?,
            crate::topology::edge::end_vertex(this.target, edge)?,
        ];
        edges.push((edge, ends));
    }
    Ok((rails, edges, tol))
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

/// The fork's `SmFilletPatchStitchCallback` (tangency-labs/smlib@1072d1b; WR-546, #1377):
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
        let null = || {
            null_defect(
                OP,
                "the stitched intermediate body's edges carry curves and its faces surfaces, as the default choice measured them",
            )
        };
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
                let end_patch = nx1
                    && (corner.edges.len() == 3 || corner.wide_cap())
                    && corner.extended_surface_patch;
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
                        return Err(SolidError::blend(
                            "SmBlendCorner::CreateEndPatchOfEnd",
                            crate::error::blend::BlendFailure::CornerRefused,
                        ));
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
    fn faces_bound_open_ends(
        &mut self,
        k: &mut Kernel,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        self.bound_open_fillet_ends(k, executive)
    }
    fn faces_make_face(
        &mut self,
        k: &mut Kernel,
        geom: FilletObject,
        body: FilletObject,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Geometry(id) = geom else {
            return Err(invalid("face geometry is not actually retained"));
        };
        if body != FilletObject::IntermediateBrep {
            return Err(invalid("face destination is not the intermediate body"));
        }
        let made = self.make_solver_face(k, id);
        let error = match made {
            Err(e) if !e.is_unported_or_cancelled() => e,
            made => return made,
        };
        // an End of this blend whose contact line passes over its end face takes all of the
        // blend at the size where its rail only just ends on that face: the face then has no
        // extent. Refused as too large, naming the radius that fits (noted, so the executive
        // keeps it)
        // (a lookup that fails, or a geometry without a live solver, shows no such End: the
        // face's failure stands)
        let ends = || -> Result<[Option<VertexId>; 2]> {
            let Some(sid) = self.pseudo.geom(id)?.fillet_solver() else {
                return Ok([None, None]);
            };
            let Some(solver) = self.solvers.get(sid.0 as usize).and_then(Option::as_ref) else {
                return Ok([None, None]);
            };
            let Some(eu) = solver.base().edgeuse(0)? else {
                return Ok([None, None]);
            };
            let Some(e) = crate::topology::edgeuse::p1::edge(self.target, eu)? else {
                return Ok([None, None]);
            };
            Ok([
                crate::topology::edge::start_vertex(self.target, e)?,
                crate::topology::edge::end_vertex(self.target, e)?,
            ])
        };
        let ends = match ends() {
            Ok(ends) => ends,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Err(error),
        };
        let mut error = error;
        for v in ends.into_iter().flatten() {
            let Some(corner) = session_corner_of_vertex(executive, v) else {
                continue;
            };
            let SessionObjects {
                target,
                pseudo,
                solvers,
                corners,
                extended_surfaces,
                ..
            } = self;
            error = end_passing_over_end_face(
                k,
                target,
                pseudo,
                solvers.as_slice(),
                extended_surfaces,
                corners.as_slice(),
                corner.0 as usize,
                false,
                executive,
                error,
            )?;
            if executive.error.no_room_reason()
                == Some(crate::blend::fillet_executive::NoRoomReason::EndPassesOverEndFace)
            {
                break;
            }
        }
        Err(error)
    }
}

impl SessionObjects<'_> {
    /// The face of fillet geometry `id` in the intermediate body
    /// (`SmFilletGeom::MakeFaceBrep`), with its solver.
    fn make_solver_face(
        &mut self,
        k: &mut Kernel,
        id: crate::blend::fillet_geom::p1::FilletGeomId,
    ) -> Result<()> {
        let sid = self.pseudo.geom(id)?.fillet_solver().ok_or_else(|| {
            SolidError::blend_solver(
                OP,
                crate::error::blend::SolverFailure::FilletGeomWithoutSolver { fillet_geom: id },
            )
        })?;
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
    fn continued_cliff(&self, vertex: VertexId, edge: EdgeId) -> Option<FaceId> {
        use crate::blend::blend_corner::BlendCornerEvent;
        let corner = session_corner_of_vertex(self.executive, vertex)?;
        match self.corners.get(corner.0 as usize)?.blend?.event {
            BlendCornerEvent::CliffJoin { rim, cliff } if rim.contains(&edge) => Some(cliff),
            _ => None,
        }
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
    fn rails_rolled_on_their_faces(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        _executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<bool> {
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        self.rolled_rails_on_their_faces(k, id)
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
    fn geometry_overflows_at_roll_over(
        &mut self,
        k: &mut Kernel,
        solver: FilletObject,
        left_across: &[Option<EdgeId>],
    ) -> Result<bool> {
        if self.roll_over_ends.is_empty() || left_across.is_empty() {
            return Ok(false);
        }
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)) =
            self.geometry_edge(solver)?
        else {
            return Err(invalid("solver edge is not a target edge"));
        };
        let ends = [
            crate::topology::edge::start_vertex(self.target, edge)?,
            crate::topology::edge::end_vertex(self.target, edge)?,
        ];
        let at: Vec<VertexId> = ends
            .iter()
            .flatten()
            .copied()
            .filter(|v| self.roll_over_ends.contains(v))
            .collect();
        let mut rim = Vec::new();
        for v in at {
            rim.extend(crate::blend::blend_corner::rollover::cliff_rim(
                k,
                self.target,
                v,
                edge,
            )?);
        }
        Ok(left_across
            .iter()
            .all(|side| side.is_some_and(|e| rim.contains(&e))))
    }
    fn geometry_overflows_into_material(
        &mut self,
        _k: &mut Kernel,
        solver: FilletObject,
        left_across: &[Option<EdgeId>],
        return_across: &[Option<EdgeId>],
        wraps: bool,
        boundaries_into_material: bool,
    ) -> Result<bool> {
        use crate::blend::fillet_solver::p5::edge_bounds_material;
        let FilletObject::Solver(id) = solver else {
            return Err(invalid("object is not a solver"));
        };
        if self.solver(id)?.constant_offsets()?.is_none()
            || left_across.is_empty()
            || !boundaries_into_material
        {
            return Ok(false);
        }
        if wraps {
            let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)) =
                self.geometry_edge(solver)?
            else {
                return Err(invalid("solver edge is not a target edge"));
            };
            if !crate::topology::edge::is_closed(self.target, edge)? {
                return Ok(false);
            }
        }
        for &side in left_across.iter().chain(return_across) {
            let Some(edge) = side else {
                return Ok(false);
            };
            if !crate::topology::edge::is_manifold(self.target, edge)?
                || !edge_bounds_material(self.target, edge)?
            {
                return Ok(false);
            }
        }
        Ok(true)
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
    fn geometry_corner_blended_edges(&self, corner: FilletObject) -> Result<Vec<FilletObject>> {
        let FilletObject::Corner(id) = corner else {
            return Err(invalid("object is not a corner"));
        };
        let Some(corner) = self.corners.get(id.0 as usize) else {
            return Ok(Vec::new());
        };
        let mut edges = Vec::new();
        for &edgeuse in corner.solver_eus.as_deref().unwrap_or(&[]) {
            let Some(edge) = crate::topology::edgeuse::p1::edge(self.target, edgeuse)? else {
                continue;
            };
            let edge = FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge));
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
        Ok(edges)
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
        // a sheet's edge bounds no material: its blend adds none (not the fork's, whose
        // `IsConcaveEdge` fails there)
        if !crate::blend::fillet_solver::p5::edge_bounds_material(self.target, edge)? {
            return Ok(false);
        }
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
            // a plane face, whose one closed edge's blend closing to a point is a cone
            // with its apex on the face (#17198)
            let planar = crate::topology::face::p2::surface(self.target, face)?.is_some_and(|s| {
                matches!(s.kind(), crate::surface::surface::SurfaceKind::Plane(_))
            });
            for lu in eu::faceuse_loopuses(self.target, fu, false, None)? {
                // The loop closes to a point when every edge of it has a point rail on
                // the face; it ends in that point when some blend there meets the face
                // at an angle.
                let edges = loopuse::edges(self.target, lu, None)?;
                // Not the fork's (#17198): a plane face bounded by one closed edge (a
                // cylinder's top) whose blend's rail on it is a point, by the same
                // test as a round's (the dome's), is taken whole by a cone whose apex
                // is that point: the blend meets the face there in a point, not along
                // an edge, so nothing of the face is left to meet at an angle
                let one_closed_edge = planar
                    && edges.len() == 1
                    && match edges.first().copied().flatten() {
                        Some(e) => crate::topology::edge::is_closed(self.target, e)?,
                        None => false,
                    };
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
                if closes_to_a_point && !all_tangent && !one_closed_edge {
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
        use crate::geom::global_solver::SolverOperation;
        let (rails, edges, tol) = band_rails(self, k, solvers)?;
        let apart = 10.0 * tol;
        // Not the fork's (#18823): a band lies beside its edge and, past the edge's ends, beside
        // the edges its edge runs on into tangentially, up to the sections its corners placed.
        // Where a tangent chain's blends meet, their shared section crosses the chain where a
        // rail crosses the seam between the faces, which can lie well before the edges'
        // common vertex, so the next blend's band starts beside the end of this blend's edge.
        // So each band is judged against its edge's run: the blended edges on its tangent
        // chain (`chains_among`), its ends being the run's, and it ends where its rail's kept
        // part ends.
        let edge_ids: Vec<EdgeId> = edges.iter().map(|&(e, _)| e).collect();
        let chains = crate::blend::blend_refusal::chains_among(
            k,
            self.target,
            &edge_ids,
            BAND_CHAIN_TANGENCY,
        )?;
        let mut edge_curves = Vec::new();
        for &e in &edge_ids {
            let edge = self.target.edge(e)?;
            let ivl = edge.interval;
            let curve = edge
                .curve
                .clone()
                .ok_or_else(|| invalid("filleted edge without curve"))?;
            let ends = [
                curve.evaluate_point(k, ivl.min())?,
                curve.evaluate_point(k, ivl.max())?,
            ];
            edge_curves.push((curve, ivl, ends));
        }
        // Pruning by boxes (#16645: the pair loop is quadratic in the blends). A pair
        // `(a, b)` can refuse only through a sample `p` of rail `a`'s kept part for which,
        // with `e` on edge `b` and `r` on rail `b`, `|p - e| < |e - r|`. So `p` lies
        // within `|e - r|` of `e`, and `|e - r|` is at most the diagonal of the union of
        // a box holding edge `b` and a box holding rail `b`. A box holding rail `a`
        // (so `p`) that is separated from edge `b`'s box grown by that diagonal (plus a
        // margin) on some axis leaves every `p` farther than the diagonal from every
        // `e`, so no sample of the pair can pass the band test. The boxes are
        // guaranteed bounds, never sampled extremes: the control polygon's box of a
        // B-spline (or line) with positive weights holds the whole curve (the convex
        // hull property), and a curve of any other kind is not pruned.
        let margin = 100.0 * tol + apart;
        let mut edge_boxes: Vec<Option<Extent3d>> = Vec::new();
        for &(edge, _) in &edges {
            let e = self.target.edge(edge)?;
            edge_boxes.push(e.curve.as_ref().and_then(control_hull_box));
        }
        let rail_boxes: Vec<Vec<Option<Extent3d>>> = rails
            .iter()
            .map(|own| {
                own.iter()
                    .map(|(_, curve, _)| control_hull_box(curve))
                    .collect()
            })
            .collect();
        // faces whose guards main runs at their first sample, kept as it ran them
        let mut guarded: BTreeSet<FaceId> = BTreeSet::new();
        for (a, &(edge_a, ends_a)) in edges.iter().enumerate() {
            for (b, &(edge_b, ends_b)) in edges.iter().enumerate() {
                if a == b || edge_a == edge_b {
                    continue;
                }
                if ends_a.iter().any(|v| ends_b.contains(v)) {
                    continue; // a corner trims them where they meet
                }
                let (Some(rails_a), Some(rails_b)) = (rails.get(a), rails.get(b)) else {
                    continue;
                };
                // only rails on a face the two blends share can refuse
                if !rails_a
                    .iter()
                    .any(|(fa, _, _)| rails_b.iter().any(|(fb, _, _)| fa == fb))
                {
                    continue;
                }
                // band `b`'s run: its edge and the edges of its tangent chain that it meets
                // (its band reaches past its ends only as far as the sections its corners
                // placed), not edge `a`; an end of an edge of the run that no other edge of
                // the run shares is an end of the run
                let run: Vec<usize> = (0..edges.len())
                    .filter(|&j| {
                        j == b
                            || (j != a
                                && chains.get(j).is_some()
                                && chains.get(j) == chains.get(b)
                                && edges.get(j).is_some_and(|&(_, ends_j)| {
                                    ends_j.iter().flatten().any(|v| ends_b.contains(&Some(*v)))
                                }))
                    })
                    .collect();
                let mut run_ends: Vec<(usize, [bool; 2])> = Vec::new();
                for &j in &run {
                    let Some(&(_, ends_j)) = edges.get(j) else {
                        continue;
                    };
                    let mut at_end = [true, true];
                    for (slot, v) in at_end.iter_mut().zip(ends_j) {
                        *slot = v.is_none()
                            || !run.iter().any(|&o| {
                                o != j
                                    && edges.get(o).is_some_and(|&(_, ends_o)| ends_o.contains(&v))
                            });
                    }
                    run_ends.push((j, at_end));
                }
                // the run's box, where every edge of it has one
                let mut run_box: Option<Extent3d> = None;
                let mut every_box = true;
                for &j in &run {
                    match edge_boxes.get(j).and_then(|x| x.as_ref()) {
                        Some(bx) => {
                            run_box = Some(match run_box {
                                Some(acc) => acc.union(bx),
                                None => *bx,
                            });
                        }
                        None => every_box = false,
                    }
                }
                let edge_box_b = if every_box { run_box } else { None };
                let edge_box_b = edge_box_b.as_ref();
                for (ia, (face_a, curve_a, kept_a)) in rails_a.iter().enumerate() {
                    for (ib, (face_b, curve_b, kept_b)) in rails_b.iter().enumerate() {
                        if face_a != face_b {
                            continue;
                        }
                        if guarded.insert(*face_a) {
                            // what main's first classification of a sample on this face
                            // refuses before it looks at the sample: the face's own
                            // hostile-coordinate check and its tolerance
                            crate::query::face::p3::check_face_coordinates(
                                k,
                                self.target,
                                *face_a,
                                "query::face::p1::point_3d_classify",
                            )?;
                            crate::topology::face::p2::tolerance(self.target, *face_a)?;
                        }
                        let (Some(&solver_a), Some(&solver_b)) = (solvers.get(a), solvers.get(b))
                        else {
                            return Err(invalid("band without its solver"));
                        };
                        if self.planar_bands_overlap(
                            k,
                            [solver_a, solver_b],
                            *face_a,
                            &edge_ids,
                            solvers,
                        )? {
                            return Ok(false);
                        }
                        if let (Some(eb), Some(Some(box_a)), Some(Some(box_b))) = (
                            edge_box_b,
                            rail_boxes.get(a).and_then(|r| r.get(ia)),
                            rail_boxes.get(b).and_then(|r| r.get(ib)),
                        ) {
                            let reach = eb.union(box_b).size().length() + margin;
                            let mut grown = *eb;
                            grown.expand_absolute(reach);
                            if box_a.are_disjoint(&grown, 0.0) {
                                // no sample can fall inside the band, and the rails cannot
                                // cross: `grown` holds rail `b`'s box
                                continue;
                            }
                        }
                        let ivl_a = *kept_a;
                        let ivl_b = curve_b.natural_interval();
                        // Not the fork's (#18823): rail `b`'s band ends where its kept part
                        // ends, at the sections its corners placed, not at its edge's ends.
                        // Where a tangent chain's blends meet, the shared section crosses the
                        // chain where the rail crosses the seam between the faces, which can
                        // lie well before the edges' common vertex: the next blend's band
                        // then runs on beside the end of this blend's edge. A sample whose
                        // nearest point of rail `b` is an end of its kept part lies past that
                        // section, beside the band. A closed rail (both ends one point) has
                        // no such end.
                        let band_ends_b = [
                            curve_b.evaluate_point(k, kept_b.min())?,
                            curve_b.evaluate_point(k, kept_b.max())?,
                        ];
                        let band_has_ends = (band_ends_b[0] - band_ends_b[1]).length() > tol;
                        // a rail whose kept part is one point (a round as wide as the
                        // corner it turns, its rail on the face collapsed onto the corner's
                        // centre) takes no band off this face
                        let middle_b = curve_b.evaluate_point(k, kept_b.evaluate(0.5))?;
                        if !band_has_ends && (middle_b - band_ends_b[0]).length() <= tol {
                            continue;
                        }
                        // Not the fork's (#18178): where the kept rails cross on the face they
                        // share. The samples alone can step over a short overlap (two chamfers
                        // at a narrow place, where one's corner mitre reaches a few hundredths
                        // past the other's rail). The crossings, found exactly by the curves'
                        // intersection within the solvers' tolerance, cut rail `a` into pieces
                        // that each lie wholly on one side of rail `b`, so the middle of each
                        // piece is judged too, by the same band test as the samples: a piece
                        // inside the band by more than the solvers' tolerance refuses, and rails
                        // that only touch (bands meeting at their fit, within that tolerance) do
                        // not.
                        let mut at: Vec<f64> = (1..16_u32)
                            .map(|s| ivl_a.evaluate(f64::from(s) / 16.0))
                            .collect();
                        let crossings = crate::intersect::api_intersectors::curve_crossings(
                            k, curve_a, kept_a, curve_b, kept_b, None, tol,
                        )?;
                        if !crossings.is_empty() {
                            let mut cuts = vec![ivl_a.min()];
                            cuts.extend(crossings.iter().map(|&(t, _)| t));
                            cuts.push(ivl_a.max());
                            at.extend(cuts.windows(2).filter_map(|w| match w {
                                [lo, hi] if lo < hi => Some(0.5 * (lo + hi)),
                                _ => None,
                            }));
                        }
                        for t_a in at {
                            let p = curve_a.evaluate_point(k, t_a)?;
                            // Main classifies the sample first and drops a sample off its
                            // face before any of the geometry below. The classification is
                            // the costly step, so the geometry runs first; where it fails
                            // (an error main would only reach for a sample on the face),
                            // the sample is classified at once, as main did: off the face
                            // it is dropped, on it the error is main's.
                            // the sample's foot on band `b`'s run: the nearest of its edges'.
                            // Where it cannot be found on an edge of the run (an evaluation
                            // fails, or the drop finds no point), the sample is classified at
                            // once (#19243).
                            let run_p = run_ends.iter().map(|&(j, ends)| {
                                edge_curves
                                    .get(j)
                                    .map(|edge| (edge, ends))
                                    .ok_or_else(|| band_defect(RUN_EDGE_GATHERED))
                            });
                            let foot = foot_on_run(
                                p,
                                run_p,
                                |&((curve_j, ivl_j, _), _), q| drop_onto_edge(k, curve_j, ivl_j, q),
                                |&((curve_j, _, _), _), t| curve_j.evaluate_point(k, t),
                                || sample_off_face(k, self.target, *face_a, p),
                            )?;
                            let FootOnRun::Found {
                                edge: ((_, ivl_j, [start_j, stop_j]), [end0, end1]),
                                parameter: t,
                                point: e,
                            } = foot
                            else {
                                continue;
                            };
                            // A constrained closest point can return an endpoint one
                            // parameter ulp inside the interval. Use the existing solver
                            // tolerance in geometric space, independent of the edge curve's
                            // parameterization. An end the run's next edge continues from is
                            // no end of the band.
                            let at_run_end = (end0
                                && (t <= ivl_j.min() || (e - *start_j).length() <= tol))
                                || (end1 && (t >= ivl_j.max() || (e - *stop_j).length() <= tol));
                            // Not the fork's (#18563): a sample past the run's end is judged
                            // only where that end is an end of edge `b` itself (below);
                            // otherwise it is beside the band: testing only t would extend the
                            // band's chord across the face and falsely catch a distant rail
                            // (#2903), and an end on another edge of the run (a tangent
                            // chain's) says nothing of band `b`'s width. A closed edge (both
                            // ends one point) has no end to run on past.
                            let own_end_b = edge_curves
                                .get(b)
                                .map(|(_, _, ends)| *ends)
                                .filter(|[q0, q1]| (*q0 - *q1).length() > tol)
                                .filter(|ends| ends.iter().any(|&q| (e - q).length() <= tol));
                            let past_end = match own_end_b {
                                Some(ends) if at_run_end && band_has_ends => Some(ends),
                                _ => None,
                            };
                            let on_rail = match curve_b.drop_point(
                                k,
                                &ivl_b,
                                p,
                                None,
                                crate::math::BIG_DOUBLE,
                                None,
                                SolverOperation::Minimize,
                            ) {
                                Ok(v) => v,
                                Err(error) => {
                                    if sample_off_face(k, self.target, *face_a, p)? {
                                        continue;
                                    }
                                    return Err(error);
                                }
                            };
                            if (at_run_end && past_end.is_none()) || !on_rail.success {
                                continue; // beside the band, not across it
                            }
                            let r = match curve_b.evaluate_point(k, on_rail.parameter) {
                                Ok(r) => r,
                                Err(error) => {
                                    if sample_off_face(k, self.target, *face_a, p)? {
                                        continue;
                                    }
                                    return Err(error);
                                }
                            };
                            if band_has_ends
                                && (on_rail.parameter <= kept_b.min()
                                    || on_rail.parameter >= kept_b.max()
                                    || band_ends_b.iter().any(|&q| (r - q).length() <= tol))
                            {
                                continue; // past the section that ends the band
                            }
                            if let Some([q0, q1]) = past_end {
                                // Not the fork's (#18563): `p` lies past an end of edge `b`,
                                // beside rail `b`'s kept part. Where that part runs on past
                                // the edge's end (a round running out onto a face beyond it,
                                // as onto the sphere that trims a block's corner), the band
                                // runs on with it: a sample in it refuses as one beside the
                                // edge does. Where the judgement cannot be made (an
                                // evaluation fails), the sample is classified as for its own
                                // evaluations: off the face it is dropped, on it the failure
                                // is returned (#19243).
                                let other_end = if (e - q0).length() <= (e - q1).length() {
                                    q1
                                } else {
                                    q0
                                };
                                let inside = match inside_band_past_run_end(
                                    k,
                                    curve_b,
                                    &ivl_b,
                                    PastRunEnd {
                                        end: e,
                                        other_end,
                                        sample: p,
                                        on_rail: (on_rail.parameter, r),
                                        kept: [kept_b.min(), kept_b.max()],
                                    },
                                    tol,
                                    apart,
                                ) {
                                    Ok(inside) => inside,
                                    Err(error) => {
                                        if sample_off_face(k, self.target, *face_a, p)? {
                                            continue;
                                        }
                                        return Err(error);
                                    }
                                };
                                if inside && !sample_off_face(k, self.target, *face_a, p)? {
                                    return Ok(false);
                                }
                                continue;
                            }
                            // Not the fork's (#18823): the band's section through `r` reaches
                            // the run at `r`'s own foot there, which is `p`'s where `p` lies in
                            // the band. On a run of several edges `p`'s foot can lie on another
                            // edge of it, across the face from the band (a tube's joint), so the
                            // chord is taken to `r`'s foot. Where `r`'s foot on an edge of the
                            // run cannot be found (an evaluation fails, or the drop finds no
                            // point), the sample is classified as for its own evaluations: off
                            // the face it is dropped, on it the failure is returned. No other
                            // point stands in for that foot (#19243).
                            let run_r = run_ends.iter().map(|&(j, _)| {
                                edge_curves
                                    .get(j)
                                    .ok_or_else(|| band_defect(RUN_EDGE_GATHERED))
                            });
                            let foot_r = foot_on_run(
                                r,
                                run_r,
                                |&(curve_j, ivl_j, _), q| drop_onto_edge(k, curve_j, ivl_j, q),
                                |&(curve_j, _, _), t| curve_j.evaluate_point(k, t),
                                || sample_off_face(k, self.target, *face_a, p),
                            )?;
                            let FootOnRun::Found { point: e, .. } = foot_r else {
                                continue;
                            };
                            let mut chord = e - r;
                            let width = chord.length();
                            // Not the fork's (#16654): the rail's point nearest `p` is the
                            // band's across from `e` only where it is near the rail's point
                            // across from `e` itself; a rail run on past an end where its
                            // blend shrinks to a vertex (two tubes of one radius) comes back
                            // round the far side, and its point there spans no band.
                            let across = curve_b.drop_point(
                                k,
                                &ivl_b,
                                e,
                                None,
                                crate::math::BIG_DOUBLE,
                                None,
                                SolverOperation::Minimize,
                            )?;
                            if across.success {
                                let band =
                                    (curve_b.evaluate_point(k, across.parameter)? - e).length();
                                // A sample inside the band at `e` is within `band` of `e`, and
                                // within `band` of the rail's point across from `e`; the
                                // rail's point nearest it is no further from it than that
                                // one. So the chord from `e` to that nearest point is at most
                                // `band + band` long, by the triangle through the sample: a
                                // longer one (past the margin) is to a point of the rail's
                                // run on, not across this band.
                                if width > 2.0 * band + apart {
                                    continue;
                                }
                            }
                            if !matches!(
                                width.partial_cmp(&(2.0 * apart)),
                                Some(core::cmp::Ordering::Greater)
                            ) {
                                continue;
                            }
                            chord *= 1.0 / width;
                            // in the band: between rail and edge along the chord, and no
                            // further from the edge than the band is wide (the chord's
                            // direction alone also takes a point far off across a curved
                            // face, whose offset happens to fall inside it: #2907)
                            let from_rail = (p - r).dot(chord);
                            // Not the fork's (#18823): a rail inside the band by more than
                            // the solvers' tolerance refuses. Bands overlapping by less than
                            // `apart` (ten tolerances) are not bands that only touch: the
                            // trim of their faces leaks there, so only an overlap within the
                            // tolerance passes.
                            if from_rail > tol
                                && from_rail < width - apart
                                && (p - e).length() < width
                                && !sample_off_face(k, self.target, *face_a, p)?
                            {
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

/// A sample of the band check whose foot on a band's run is an end of the band's own edge
/// ([`inside_band_past_run_end`]).
#[derive(Clone, Copy)]
struct PastRunEnd {
    /// The edge's end: the sample's foot on the run.
    end: crate::math::Point3,
    /// The edge's other end.
    other_end: crate::math::Point3,
    /// The sample.
    sample: crate::math::Point3,
    /// The point of the band's rail nearest the sample, by its parameter, inside the rail's
    /// kept part.
    on_rail: (f64, crate::math::Point3),
    /// The parameters of the ends of the rail's kept part.
    kept: [f64; 2],
}

/// The band check past an end of a band's own edge (#18563): whether the sample lies in the
/// band's run-on, the part of the band beside the rail's kept part past the edge's end. The
/// band's section at that end goes from the rail's point across from the end to the end. The
/// run-on is the rail's part from that point to the end of its kept part on the side away from
/// the edge (the side the point across from the edge's other end does not lie on). The sample
/// is in the run-on's band where its nearest point of the rail lies strictly within the
/// run-on, and it lies on the end's side of the rail in the end section's direction, further
/// than `tol` from the rail and short of that section's width by more than `apart`, as the
/// check beside the edge judges a sample. The section's direction is the end's, not the rail's
/// at the sample's nearest point: the two agree where the run-on is straight and stays on a
/// plane. A rail without a point across from either end, or a section no wider than `2 apart`,
/// takes no band there.
///
/// # Errors
/// Those of the drops onto the rail and its evaluation.
fn inside_band_past_run_end(
    k: &Kernel,
    rail: &crate::curve::curve::Curve,
    ivl: &crate::geom::extent1d::Extent1d,
    at: PastRunEnd,
    tol: f64,
    apart: f64,
) -> Result<bool> {
    use crate::geom::global_solver::SolverOperation;
    let PastRunEnd {
        end,
        other_end,
        sample,
        on_rail: (t, r),
        kept: [t0, t1],
    } = at;
    let drop = |q| {
        rail.drop_point(
            k,
            ivl,
            q,
            None,
            crate::math::BIG_DOUBLE,
            None,
            SolverOperation::Minimize,
        )
    };
    let across = drop(end)?;
    let across_other = drop(other_end)?;
    if !across.success || !across_other.success {
        return Ok(false);
    }
    // the run-on: from the point across from the end to the kept end on the side away from
    // the point across from the edge's other end
    let (lo, hi) = if across_other.parameter <= across.parameter {
        (across.parameter, t1)
    } else {
        (t0, across.parameter)
    };
    if !(t > lo && t < hi) {
        return Ok(false);
    }
    let section = end - rail.evaluate_point(k, across.parameter)?;
    let width = section.length();
    if !matches!(
        width.partial_cmp(&(2.0 * apart)),
        Some(core::cmp::Ordering::Greater)
    ) {
        return Ok(false);
    }
    let from_rail = (sample - r).dot(section * (1.0 / width));
    Ok(from_rail > tol && from_rail < width - apart && (sample - r).length() < width)
}

/// A step of the band check that found nothing where a point always exists (a drop onto a
/// run edge that returns no point, a run without its band's edge): a reviewed
/// [`crate::error::blend::BlendFailure::Defect`], `why` saying why the point exists, since a
/// search that stops proves nothing about the geometry. Status `SM_ERR`; its origin is the
/// caller's line.
#[track_caller]
fn band_defect(why: &'static str) -> SolidError {
    defect("SmFilletExecutive::BandsClearOtherRails (fork)", why)
}

/// Why a band's run holds each edge it names: the run is built from its band's edges, whose
/// curves the check gathers before it walks the run.
const RUN_EDGE_GATHERED: &str =
    "a band's run is built from its band's edges, whose curves the check gathers first";

/// A point's foot on a band's run, as the band check finds it ([`foot_on_run`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum FootOnRun<E> {
    /// The nearest of the point's feet on the run's edges: the edge it lies on, its
    /// parameter there and the foot.
    Found {
        edge: E,
        parameter: f64,
        point: crate::math::Point3,
    },
    /// A foot could not be found on an edge of the run, and the band check's sample lies
    /// off its face: the sample is dropped.
    SampleOffFace,
}

/// The band check's search for `point`'s foot on a band's run (`run`, one entry per edge):
/// on each edge, the closest point (`drop_onto`) evaluated at its parameter (`evaluate`),
/// and the nearest of those.
///
/// Where a foot cannot be found on an edge (a missing edge, a drop or an evaluation that
/// fails, or a drop that finds no point), nothing stands in for it, neither another edge's
/// foot nor another point (#19243). The sample is classified as for its other evaluations
/// (`off_face`): off its face it is dropped ([`FootOnRun::SampleOffFace`]), on it the failure
/// is returned. A drop that finds no point, and a run without edges, are a
/// [`band_defect`]: a bounded curve always has a closest point, and the run
/// always holds its band's edge.
///
/// # Errors
/// Those of `run`, `drop_onto`, `evaluate` and `off_face`, for a sample on its face.
fn foot_on_run<E>(
    point: crate::math::Point3,
    run: impl IntoIterator<Item = Result<E>>,
    mut drop_onto: impl FnMut(&E, crate::math::Point3) -> Result<crate::curve::curve::p6::DroppedPoint>,
    mut evaluate: impl FnMut(&E, f64) -> Result<crate::math::Point3>,
    off_face: impl FnOnce() -> Result<bool>,
) -> Result<FootOnRun<E>> {
    let mut nearest: Option<(f64, E, f64, crate::math::Point3)> = None;
    for edge in run {
        let foot_on_edge = || -> Result<(E, f64, crate::math::Point3)> {
            let edge = edge?;
            let on = drop_onto(&edge, point)?;
            if !on.success {
                return Err(band_defect("a bounded curve always has a closest point"));
            }
            let q = evaluate(&edge, on.parameter)?;
            Ok((edge, on.parameter, q))
        };
        match foot_on_edge() {
            Ok((edge, parameter, q)) => {
                let dist = (point - q).length();
                if nearest.as_ref().is_none_or(|&(best, ..)| dist < best) {
                    nearest = Some((dist, edge, parameter, q));
                }
            }
            Err(error) => {
                if off_face()? {
                    return Ok(FootOnRun::SampleOffFace);
                }
                return Err(error);
            }
        }
    }
    match nearest {
        Some((_, edge, parameter, point)) => Ok(FootOnRun::Found {
            edge,
            parameter,
            point,
        }),
        None => Err(band_defect("a band's run always holds its band's edge")),
    }
}

/// The closest point to `point` of a band run's edge curve over `interval` (the band check's
/// drop).
fn drop_onto_edge(
    k: &Kernel,
    curve: &crate::curve::curve::Curve,
    interval: &crate::geom::extent1d::Extent1d,
    point: crate::math::Point3,
) -> Result<crate::curve::curve::p6::DroppedPoint> {
    curve.drop_point(
        k,
        interval,
        point,
        None,
        crate::math::BIG_DOUBLE,
        None,
        crate::geom::global_solver::SolverOperation::Minimize,
    )
}

/// Whether `p` lies off `face` (the band check's classification of a sample).
fn sample_off_face(
    k: &Kernel,
    target: &mut Brep,
    face: FaceId,
    p: crate::math::Point3,
) -> Result<bool> {
    use crate::intersect::point_class::{PointClassType, PointClassification};
    let face_tol =
        crate::geom::tol::zone_tol3d(k, crate::topology::face::p2::tolerance(target, face)?);
    let mut on = PointClassification::<EntityRef>::new(face_tol);
    crate::query::face::p1::point_3d_classify(
        k,
        target,
        face,
        p,
        face_tol,
        true,
        &mut on,
        crate::query::face::p1::Point3dClassifyOptions::default(),
    )?;
    Ok(on.point_class() == PointClassType::Unknown)
}

/// The box of a curve's control polygon when that holds the whole curve for certain: a
/// B-spline or line whose points are finite and whose weights, if rational, are positive
/// (the convex hull property). Any other curve has none.
fn control_hull_box(curve: &crate::curve::curve::Curve) -> Option<Extent3d> {
    use crate::curve::curve::CurveKind;
    let (points, weights) = match curve.kind() {
        CurveKind::BSpline(c) => c.control_polygon().ok()?,
        CurveKind::Line(l) => l.base_with_nurb().ok()?.control_polygon().ok()?,
        _ => return None,
    };
    if weights.iter().any(|w| !(*w > 0.0 && w.is_finite())) {
        return None;
    }
    let mut it = points.iter();
    let first = *it.next()?;
    let mut hull = Extent3d::from_point(first);
    for &p in std::iter::once(&first).chain(it) {
        if !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite()) {
            return None;
        }
        hull.add_point(p);
    }
    Some(hull)
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
            .ok_or_else(|| edge_without_curve(LOADER, id))?;
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
                    crate::error::Failure::new(
                        LOADER,
                        crate::error::blend::BlendFailure::Solver(
                            crate::error::blend::SolverFailure::EdgeCurveCacheNotBuilt { edge: id },
                        ),
                    )
                    .caused_by(error)
                    .into_error()
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
    fn parameters_edge_tolerance(&mut self, k: &Kernel, edge: FilletObject) -> Result<f64> {
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)) = edge else {
            return Err(invalid("edge is not in the target"));
        };
        // not upstream: the fit tolerance (the edge tolerance unless that only covers the
        // edge's edge/face gap)
        crate::topology::edgeuse::p3::fit_tolerance(k, self.target, id)
    }
    fn parameters_edge_gap(&mut self, k: &Kernel, edge: FilletObject) -> Result<f64> {
        let FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)) = edge else {
            return Err(invalid("edge is not in the target"));
        };
        let mut gap = None::<f64>;
        // The gap only decides a tolerance at the B-rep's scale: a face that is a blend by its
        // definition is measured on its certified net wherever the net's bound is far inside
        // that (a reopened round's definition is otherwise evaluated at every sample, as no
        // scope is open yet).
        let _scope =
            crate::surface::procedural_blend::NetScope::within(k, 0.1 * self.target.tolerance());
        for eu in self.target.edge(id)?.edgeuses.clone() {
            let measured =
                crate::topology::edgeuse::p1::max_edge_face_gap(k, self.target, eu, false)?;
            if let Some(g) = measured.filter(|g| g.gap_3d >= 0.0) {
                gap = Some(gap.map_or(g.gap_3d, |m| m.max(g.gap_3d)));
            }
        }
        match gap {
            Some(gap) => Ok(gap),
            None => crate::topology::edge::tolerance(self.target, id),
        }
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
                let v = v.ok_or_else(|| {
                    SolidError::blend_solver(
                        OP,
                        crate::error::blend::SolverFailure::EdgeWithoutVertex { edge: id },
                    )
                })?;
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
    fn parameters_runs_on_across_seams(
        &mut self,
        k: &Kernel,
        edges: &[FilletObject],
        ang_tol_deg: f64,
    ) -> Result<Vec<RunOn<FilletObject>>> {
        let mut ids = Vec::new();
        for &edge in edges {
            match edge {
                FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)) => ids.push(id),
                _ => return Err(invalid("edge is not in the target")),
            }
        }
        Ok(runs_on_across_seams(k, self.target, &ids, ang_tol_deg)?
            .into_iter()
            .map(|run_on| RunOn {
                source: run_on.source,
                edge: FilletObject::Topology(FilletBody::Target, EntityRef::Edge(run_on.edge)),
                flipped: run_on.flipped,
            })
            .collect())
    }
}

/// The edges of `target` that continue `edges` across seams' vertices
/// ([`FilletParametersAccess::parameters_runs_on_across_seams`]): from each end of each
/// picked edge, each next piece of its curve while a seam's vertex splits it, up to
/// `SIDE_EDGE_CHAIN_STEPS` pieces, each named once and none of them picked.
///
/// # Errors
/// Those of the topology walks and the seam and tangency tests.
pub(crate) fn runs_on_across_seams(
    k: &Kernel,
    target: &mut Brep,
    edges: &[EdgeId],
    ang_tol_deg: f64,
) -> Result<Vec<RunOn<EdgeId>>> {
    use crate::blend::blend_corner::end::{side_edgeuse_across_seam_vertex, SIDE_EDGE_CHAIN_STEPS};
    use crate::topology::edgeuse::p1 as eu1;
    let mut taken: BTreeSet<EdgeId> = edges.iter().copied().collect();
    let mut run_ons = Vec::new();
    for (source, &id) in edges.iter().enumerate() {
        // any of its edge uses walks the curve (the blend edge use, which evaluates the
        // faces, is chosen only for an edge that runs on)
        let Some(&eu) = crate::topology::edge::edgeuses(target, id, None, None)?.first() else {
            continue;
        };
        let mut face = None;
        let (start, end) = crate::topology::vertex::edge_vertex_pair(target, id)?;
        for at in [start, end].into_iter().flatten() {
            // along the curve from `at`: each next piece while a seam's vertex splits it
            let (mut curr, mut at) = (eu, at);
            for _ in 0..SIDE_EDGE_CHAIN_STEPS {
                let Some(next) = side_edgeuse_across_seam_vertex(k, target, curr, at, ang_tol_deg)?
                else {
                    break;
                };
                let next_edge =
                    eu1::edge(target, next)?.ok_or_else(|| invalid("edge use without edge"))?;
                if !taken.insert(next_edge) {
                    break;
                }
                // the faces each blend edge use lies on; an edge with none is refused when
                // its solver is made
                if face.is_none() {
                    face = match crate::topology::edge::blend_edgeuse(k, target, id)? {
                        Some(u) => Some(eu1::face(target, u)?),
                        None => Some(None),
                    };
                }
                let blended = crate::topology::edge::blend_edgeuse(k, target, next_edge)?;
                let flipped = match (blended, face.flatten()) {
                    (Some(u), Some(f)) => eu1::face(target, u)? != Some(f),
                    _ => false,
                };
                run_ons.push(RunOn {
                    source,
                    edge: next_edge,
                    flipped,
                });
                let (s, e) = crate::topology::vertex::edge_vertex_pair(target, next_edge)?;
                let Some(far) = (if s == Some(at) { e } else { s }) else {
                    break;
                };
                (curr, at) = (next, far);
            }
        }
    }
    Ok(run_ons)
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
            return Err(null_defect(
                "SmFilletNx1Corner::MakeCornerTopology",
                SOLVER_OF_BLENDED_EDGEUSE,
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
        // Not upstream: at a cusp the band can run off its face before the vertex. Where the
        // topology then fails ("Unable to Make Topology"), that is given as no room. Read before
        // the topology is made and used only when it fails, so a corner that builds is never refused.
        let cusp = {
            let corner = corners
                .get(id.0 as usize)
                .ok_or_else(|| invalid("corner is not live"))?;
            // Infallible: a lookup that fails here only means no cusp note, never a refusal.
            if crate::blend::fillet_corner::topology::cusp_side_edge(k, target, corner)
                .ok()
                .flatten()
                .is_some()
            {
                let vertex = corner.vertex;
                let location = vertex.and_then(|v| crate::topology::vertex::point(target, v).ok());
                let edge = crate::topology::edgeuse::p1::edge(target, edgeuse)
                    .ok()
                    .flatten();
                Some((vertex, location, edge))
            } else {
                None
            }
        };
        let solver = solvers
            .get_mut(solver.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("corner solver is not live"))?;
        // A chamfer of unequal offsets is sized by a scale, any other blend by a radius.
        let fit_kind = match solver.base().chamfer {
            Some(chamfer)
                if chamfer
                    .offsets()
                    .is_ok_and(|[r1, _]| !chamfer.holds([r1, r1])) =>
            {
                crate::blend::fillet_executive::p3::FitKind::Scale
            }
            _ => crate::blend::fillet_executive::p3::FitKind::Radius,
        };
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
        // On a cusp corner, a failure of the topology or of its end-gap checks (where the
        // cusp case fails) is given as the cusp.
        let made = match corner.make_nx1_corner_topology(env) {
            Ok(Some(gaps)) => self.nx1_end_gap_checks(k, id, gaps, executive),
            Ok(None) => Ok(()),
            Err(e) => Err(e),
        };
        match (made, cusp) {
            (Err(e), _) if e.is_unported_or_cancelled() => Err(e),
            (Err(cause), Some((vertex, location, edge))) => {
                executive.error.note_vertex_error(
                    vertex
                        .map(|v| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))),
                    location,
                    crate::blend::fillet_geom::p1::FilStatus::Failure,
                    crate::blend::fillet_geom::p1::FilletErrorType::NoRoom,
                    Some(NX1_CUSP_SIDE_EDGE),
                );
                executive.error.no_room =
                    Some(crate::blend::fillet_executive::NoRoomReason::RailLeavesFace);
                executive.note_fit_radius(
                    0.0,
                    edge.map(|e| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e))),
                    fit_kind,
                );
                Err(crate::error::Failure::new(
                    "SmFilletNx1Corner::MakeCornerTopology",
                    crate::error::blend::BlendFailure::NoRoom(
                        crate::blend::fillet_executive::NoRoomReason::RailLeavesFace,
                    ),
                )
                .caused_by(cause)
                .into_error())
            }
            (made, _) => made,
        }
    }

    /// The closed Turn's topology (#2349) with the solver of its one filleted edge use.
    fn make_closed_turn_corner_topology(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &FilletExecutive<FilletObject>,
    ) -> Result<()> {
        const OP_TURN: &str = "blend::fillet_corner::topology::make_closed_turn_topology";
        let edgeuse = self
            .corners
            .get(id.0 as usize)
            .and_then(|corner| corner.solver_eus.as_deref())
            .and_then(|edgeuses| edgeuses.first())
            .copied()
            .ok_or_else(|| invalid("corner has no actual solver edge use"))?;
        let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(edgeuse));
        let Some(FilletObject::Solver(solver)) = executive.fillet_solver_of_edgeuse(object) else {
            return Err(null_defect(OP_TURN, SOLVER_OF_BLENDED_EDGEUSE));
        };
        let SessionObjects {
            target,
            pseudo,
            solvers,
            corners,
            ..
        } = self;
        let solver = solvers
            .get_mut(solver.0 as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| invalid("corner solver is not live"))?;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        corner.make_closed_turn_topology(
            crate::blend::fillet_corner::topology::closed_turn::ClosedTurnTopologyEnv {
                k,
                target,
                pseudo,
                corner: id,
                solver,
            },
        )
    }
    /// The end of `SmFilletNx1Corner::MakeCornerTopology` for a mixed-convexity end
    /// face spanning several faces: the corner's vertex geometry and the two new side
    /// edges' geometry (statuses ignored, as upstream), then the coincidence of the
    /// kept extended end surface with the end cap's piece of the other side edge.
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
        self.calc_nx1_corner_vert_geom(k, id)?;

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
            result?;
        }

        // When proposing an extended surface
        let Some(face) = gaps.extended_face else {
            return Ok(());
        };
        let other = gaps.other_side_edgeuse.ok_or_else(|| {
            null_defect(
                "SmFilletNx1Corner::MakeCornerTopology",
                GAP_EXTENSION_SET_TOGETHER,
            )
        })?;
        let other_edge = crate::topology::edgeuse::p1::edge(self.target, other)?
            .ok_or_else(|| edgeuse_without_edge("SmFilletNx1Corner::MakeCornerTopology", other))?;
        let mut curve =
            crate::topology::edge::curve(self.target, other_edge)?.ok_or_else(|| {
                edge_without_curve("SmFilletNx1Corner::MakeCornerTopology", other_edge)
            })?;
        let mut interval = crate::topology::edge::interval(self.target, other_edge)?;
        // (not upstream, #15505) The end cap meets OtherSideEdge only along its own edge
        // there, from the vertex to the rail's crossing, whose curve the loop above set:
        // that piece must lie on the extended surface, not the whole edge (upstream tests
        // the whole edge, so an edge running on past the extension gets a G0 corner patch
        // although the cap lies on the extended surface). Where that edge got no curve
        // (its geometry's status is ignored above) the whole edge is tested, as upstream.
        for edge in [gaps.edge1, gaps.edge2] {
            if self.pseudo.fillet_edge(edge)?.original_edge() != Some(other_edge) {
                continue;
            }
            if let Some(piece) = crate::topology::edge::curve(self.pseudo.brep(), edge)? {
                curve = piece;
                interval = piece.natural_interval();
            }
        }
        let surface = self.extended_surfaces.get(&face).ok_or_else(|| {
            null_defect(
                "SmFilletNx1Corner::MakeCornerTopology",
                GAP_EXTENSION_SET_TOGETHER,
            )
        })?;
        let corner = self
            .corners
            .get(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        // get extendedSurface/OtherSideEdge coincidence to within tolerance (fork
        // 66b02de passes this on: SER)
        let found = surface
            .simple_coincidence_checker::<crate::topology::ObjectHandle>(
                k,
                &surface.natural_uv_domain_with_nurb(k)?,
                curve,
                &interval,
                corner.approx_tol_3d,
                false,
                [None, None],
            )?
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
        // Not the fork's (#2349): a closed edge meeting itself at an angle is a Turn of its
        // one blend, Mitred, with its one solver
        if self
            .corners
            .get(id.0 as usize)
            .is_some_and(FilletCorner::is_closed_turn)
        {
            return self.make_closed_turn_corner_topology(k, id, executive);
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
                return Err(null_defect(
                    "SmFilletNx2Corner::MakeCornerTopology",
                    SOLVER_OF_BLENDED_EDGEUSE,
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
            .ok_or_else(|| {
                null_defect(
                    "SmBlendCorner::MakeBallTopology",
                    "a ball corner's edge uses are set when the corner is classified, before its topology is made",
                )
            })?;
        let mut solver_ids = vec![FilletSolverId(0); all.len()];
        for (slot, index) in solver_ids.iter_mut().zip(0..) {
            let eu = all.get(index).copied().ok_or_else(|| {
                defect(
                    "SmBlendCorner::MakeBallTopology",
                    "the index runs over the corner's edge uses",
                )
            })?;
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
            *slot = found.ok_or_else(|| {
                null_defect("SmBlendCorner::MakeBallTopology", SOLVER_OF_BLENDED_EDGEUSE)
            })?;
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
            JoinTopology, JoinTopologyEnv, JOIN_ACROSS_EDGES, JOIN_UNEQUAL_OFFSETS,
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
                .ok_or_else(|| null_defect("SmBlendCorner::MakeJoinTopology", JOIN_EDGEUSES))?;
            let edge = crate::topology::edgeuse::p1::edge(target, first)?;
            executive.note_fit_radius(
                0.0,
                edge.map(|e| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e))),
                crate::blend::fillet_executive::p3::FitKind::Radius,
            );
            return Err(SolidError::blend(
                "SmBlendCorner::MakeJoinTopology",
                crate::error::blend::BlendFailure::CornerRefused,
            ));
        }
        // (#2905) the size that builds: the other blend's offset, on the wider one's edge
        if let JoinTopology::RefusedUnequalOffsets {
            blend,
            builds,
            kind,
        } = built
        {
            let location = match vertex {
                Some(v) => crate::topology::vertex::point(target, v).ok(),
                None => None,
            };
            executive.error.note_vertex_error(
                vertex.map(|v| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))),
                location,
                crate::blend::fillet_geom::p1::FilStatus::Failure,
                crate::blend::fillet_geom::p1::FilletErrorType::NoRoom,
                Some(JOIN_UNEQUAL_OFFSETS),
            );
            executive.error.no_room =
                Some(crate::blend::fillet_executive::NoRoomReason::JoinOffsetsDiffer);
            let wider = corners
                .get(id.0 as usize)
                .and_then(|c| c.solver_eus.as_deref())
                .and_then(|eus| eus.get(blend).copied())
                .ok_or_else(|| null_defect("SmBlendCorner::MakeJoinTopology", JOIN_EDGEUSES))?;
            let edge = crate::topology::edgeuse::p1::edge(target, wider)?;
            executive.note_fit_radius(
                builds,
                edge.map(|e| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e))),
                kind,
            );
            return Err(SolidError::blend(
                "SmBlendCorner::MakeJoinTopology",
                crate::error::blend::BlendFailure::NoRoom(
                    crate::blend::fillet_executive::NoRoomReason::JoinOffsetsDiffer,
                ),
            ));
        }
        Ok(())
    }

    /// Not the fork's (#17224): the vertex geometry of corner `id` failed with `error`. Where
    /// it is a Join whose round's contact line never reaches the edge between its faces
    /// ([`FilletCorner::join_rail_misses_side_edge`]), the round is refused as too large:
    /// `SM_FILERR_NO_ROOM` noted at the vertex
    /// ([`crate::blend::fillet_corner::topology::join::JOIN_RAIL_MISSES_SIDE_EDGE`]), the
    /// fit on the first blend's edge (the largest ball touching the side edge, less the fork's
    /// `FitMargin`, a thousand of that edge's tolerances, as the other open fits: at that
    /// radius the contact line only touches the edge), and the returned error the typed
    /// [`crate::error::blend::BlendFailure::NoRoom`]
    /// ([`super::NoRoomReason::JoinRailMissesSideEdge`]) carrying that fit. Anything else
    /// returns `error` as it is. Asked only after the failure, so it never refuses a Join
    /// that builds.
    ///
    /// # Errors
    /// Those of the side-edge check and of the topology queries.
    fn join_rail_missing_side_edge(
        &mut self,
        k: &Kernel,
        id: FilletCornerId,
        executive: &mut FilletExecutive<FilletObject>,
        error: SolidError,
    ) -> Result<SolidError> {
        use crate::blend::fillet_corner::topology::join::JOIN_RAIL_MISSES_SIDE_EDGE;
        const OP_JOIN: &str = "SmBlendCorner::CalcCornerVertGeom";
        let Some(corner) = self.corners.get(id.0 as usize) else {
            return Ok(error);
        };
        if !corner
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_join)
        {
            return Ok(error);
        }
        let mut solver_eu_solvers = Vec::new();
        for &eu in corner.solver_eus.as_deref().unwrap_or_default() {
            let object = FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(eu));
            solver_eu_solvers.push(match executive.fillet_solver_of_edgeuse(object) {
                Some(FilletObject::Solver(solver)) => Some(solver),
                _ => None,
            });
        }
        let Some(builds) = corner.join_rail_misses_side_edge(
            k,
            &mut *self.target,
            &self.solvers,
            &solver_eu_solvers,
        )?
        else {
            return Ok(error);
        };
        let Some(first) = corner
            .solver_eus
            .as_deref()
            .and_then(|eus| eus.first().copied())
        else {
            return Ok(error);
        };
        let vertex = corner.filleted_vertex();
        let location = match vertex {
            Some(v) => crate::topology::vertex::point(self.target, v).ok(),
            None => None,
        };
        executive.error.note_vertex_error(
            vertex.map(|v| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))),
            location,
            crate::blend::fillet_geom::p1::FilStatus::Failure,
            crate::blend::fillet_geom::p1::FilletErrorType::NoRoom,
            Some(JOIN_RAIL_MISSES_SIDE_EDGE),
        );
        executive.error.no_room =
            Some(crate::blend::fillet_executive::NoRoomReason::JoinRailMissesSideEdge);
        let edge = crate::topology::edgeuse::p1::edge(self.target, first)?;
        let margin = match edge {
            Some(e) => 1000.0 * crate::topology::edge::tolerance(self.target, e)?,
            None => 0.0,
        };
        let fit = (builds - margin).max(0.0);
        let edge_object =
            edge.map(|e| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(e)));
        executive.note_fit_radius(
            fit,
            edge_object,
            crate::blend::fillet_executive::p3::FitKind::Radius,
        );
        let mut failure = crate::error::Failure::new(
            OP_JOIN,
            crate::error::blend::BlendFailure::NoRoom(
                crate::blend::fillet_executive::NoRoomReason::JoinRailMissesSideEdge,
            ),
        )
        .with_fit(crate::error::Fit::new(
            fit,
            crate::error::FitMeasure::Radius,
            edge_object
                .as_ref()
                .and_then(crate::error::TargetEdge::of_fillet_object),
        ));
        if let Some(v) = vertex {
            failure = failure.at(EntityRef::Vertex(v));
        }
        Ok(failure.into_error())
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
    /// `MakeCornerTopology` of a 4x3, open or degenerate corner, with the
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
            FilletCornerType::FourByThree => corner.make_4x3_corner_topology(env),
            FilletCornerType::Open => corner.make_open_corner_topology(env),
            FilletCornerType::Degenerate => corner.make_degenerate_corner_topology(env),
            _ => Err(invalid("not a 4x3, open or degenerate corner")),
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
                return Err(null_defect(
                    "SmFillet3x2MixedConvexityCorner::MakeCornerTopology",
                    SOLVER_OF_BLENDED_EDGEUSE,
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
        // Chamfers of both convexities at a vertex of three, as a bevel: the bevel looks for
        // the chamfers' common point from a rails' crossing towards the vertex, and here it
        // lies across the vertex's faces. Refused, at the vertex, until the chamfer event
        // builds their mitre.
        if let Some(corner) = self.corners.get(id.0 as usize) {
            if corner.bevel && corner.chamfer_mixed_convexity {
                let vertex = corner.filleted_vertex();
                let location =
                    vertex.and_then(|v| crate::topology::vertex::point(self.target, v).ok());
                executive.error.note_vertex_error(
                    vertex
                        .map(|v| FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(v))),
                    location,
                    crate::blend::fillet_geom::p1::FilStatus::Failure,
                    crate::blend::fillet_geom::p1::FilletErrorType::CornerUnsupported,
                    Some(MIXED_CHAMFER_CORNER),
                );
                return Err(SolidError::Unsupported {
                    operation: NATIVE,
                    reason: MIXED_CHAMFER_CORNER_REASON,
                });
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
            return Err(null_defect(NATIVE, SOLVER_OF_BLENDED_EDGEUSE));
        }
        let mut solver_ids = Vec::new();
        for &eu in edgeuses {
            let solver = if let Some(solver) = lookup(eu)? {
                solver
            } else {
                let mate =
                    crate::topology::edgeuse::p1::mate(self.target, eu)?.ok_or_else(|| {
                        SolidError::blend_solver(
                            NATIVE,
                            crate::error::blend::SolverFailure::EdgeUseWithoutMate,
                        )
                    })?;
                lookup(mate)?.ok_or_else(|| null_defect(NATIVE, SOLVER_OF_BLENDED_EDGEUSE))?
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

/// The Nx1 corner refused at a cusp (solidcraft#13156).
const NX1_CUSP_SIDE_EDGE: &str = "Fillet Error: the blend's band runs off its face, which comes to a point at the vertex (a side edge leaves it along the blended edge)";

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
            FilletCornerType::NByOne => self.make_nx1_corner_topology(k, id, executive),
            FilletCornerType::NByTwo => self.make_nx2_corner_topology(k, id, executive),
            FilletCornerType::NByNConvex => self.make_convex_nxn_corner_topology(k, id, executive),
            FilletCornerType::NByNConcave => {
                self.make_concave_nxn_corner_topology(k, id, executive)
            }
            FilletCornerType::ThreeByTwoMixed => {
                self.make_3x2_mixed_corner_topology(k, id, executive)
            }
            FilletCornerType::FourByThree
            | FilletCornerType::Open
            | FilletCornerType::Degenerate => self.make_end_corner_topology(k, id, executive),
            // Abstract upstream (pure virtual): no corner object has these types; nor the 1x1
            // and 2x2 corners', whose classes the blend corner's Join replaced (#2400), nor
            // the closed Nx1 corner's, which its Join and closed Turn replaced (#2349).
            FilletCornerType::NByOneClosed
            | FilletCornerType::Unknown
            | FilletCornerType::NByN
            | FilletCornerType::OneByOne
            | FilletCornerType::TwoByTwo => Err(invalid("no corner object has this type")),
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
                    .ok_or_else(|| null_defect(OP, SOLVER_OF_BLENDED_EDGEUSE))
            })
            .collect()
    }
    fn vertices_solver_edge(&self, solver: FilletObject) -> Result<FilletObject> {
        let eu = self
            .load_solver_edgeuse(solver, 0)?
            .ok_or_else(|| null_defect(OP, SOLVER_ON_ITS_EDGEUSE))?;
        self.load_edgeuse_edge(eu)?.ok_or_else(|| match eu {
            FilletObject::Topology(FilletBody::Target, EntityRef::Edgeuse(id)) => {
                edgeuse_without_edge(OP, id)
            }
            _ => SolidError::blend(OP, crate::error::blend::BlendFailure::EdgeUseWithoutEdge),
        })
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
            .ok_or_else(|| edge_without_curve(OP, id))?;
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
        let rolled_end = {
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
        match self.calc_nx1_corner_vert_geom_rolling(k, id, rolled_end) {
            Err(e) if !e.is_unported_or_cancelled() => {
                let e = self.join_rail_missing_side_edge(k, id, executive, e)?;
                let SessionObjects {
                    target,
                    pseudo,
                    solvers,
                    corners,
                    extended_surfaces,
                    ..
                } = self;
                Err(end_passing_over_end_face(
                    k,
                    target,
                    pseudo,
                    solvers.as_slice(),
                    extended_surfaces,
                    corners.as_slice(),
                    id.0 as usize,
                    true,
                    executive,
                    e,
                )?)
            }
            result => result,
        }
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
            extended_surfaces,
            ..
        } = self;
        let corner = corners
            .get_mut(id.0 as usize)
            .ok_or_else(|| invalid("corner is not live"))?;
        let mut context = SessionNx2Adjust {
            k,
            target,
            pseudo,
            corner: id,
            solvers: solvers.as_mut_slice(),
            extended_surfaces,
        };
        corner.adjust_topology_with(
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
        )
    }
}

/// What a corner whose blend classification failed with `cause` at `vertex` of `target` gives
/// (`classify_ball`, `classify_setback`): an unsupported corner that keeps the classification's
/// error (#19803), except for a cancellation, an unported call or a certificate out of its work
/// budget (`is_unported_or_cancelled`), which is passed on as it is, as [`super::p3::caught`] and
/// the setback-request classification do (#20216): work given up is never reported as a corner
/// the kernel does not support.
///
/// # Errors
///
/// `cause`, where it is unported, cancelled or not certified.
fn unclassified_corner<O>(
    target: &Brep,
    vertex: VertexId,
    cause: SolidError,
) -> Result<CornerCreation<O>> {
    if cause.is_unported_or_cancelled() {
        return Err(cause);
    }
    let location = crate::topology::vertex::point(target, vertex).ok();
    Ok(CornerCreation::Unsupported {
        location,
        cause: Some(cause),
    })
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

    /// Only the constant-radius solvers have fixed offsets an End's rail end can be solved
    /// with (#15906): a constant-distance solver solves its offset at each section and a
    /// variable-radius one takes it from its law.
    #[test]
    fn only_constant_radius_solvers_have_constant_offsets() {
        use crate::blend::fillet_standard_solver::p1::{
            CurveBasedFs, FilletLawKind, VariableRadiusFs,
        };
        let constant_distance = SolverObject::ConstantDistance(Box::new(ConstantDistanceFs {
            solver: FilletSolver::new(1e-6, 0.5, 0.03),
            distance: 2.0,
        }));
        assert_eq!(constant_distance.constant_offsets().unwrap(), None);
        let variable = SolverObject::VariableRadius(Box::new(VariableRadiusFs {
            curve_based: CurveBasedFs::new(1e-6, 0.5, 0.03),
            law: FilletLaw::new(FilletLawKind::Linear {
                start_radius: 1.0,
                end_radius: 2.0,
            }),
            law_orientation: false,
            law_interval: crate::geom::extent1d::Extent1d::from_min_max(0.0, 1.0),
        }));
        assert_eq!(variable.constant_offsets().unwrap(), None);
    }

    /// An empty edge list is the named cause `NoEdgesGiven`, a refusal of the input, with the
    /// status of the bare `SM_ERR_INVALID_INPUT` it replaced (#13790), for every prepare that
    /// checks it.
    #[test]
    fn an_empty_edge_list_is_a_typed_invalid_value() {
        let mut k = Kernel::default();
        let mut body = pentagonal_pyramid(&mut k);
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        let errors = [
            session
                .prepare_constant_radius_edges(&mut k, &[], 0.5, 1, 1, 1.0)
                .unwrap_err(),
            session
                .prepare_per_edge_constant_radius(&mut k, &[], &[], &[], 1, 1.0)
                .unwrap_err(),
        ];
        for e in errors {
            assert_eq!(
                e.blend_failure(),
                Some(&crate::error::blend::BlendFailure::NoEdgesGiven),
                "{e:?}"
            );
            assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput);
            assert_eq!(
                e.upstream_status(),
                Some(crate::error::SM_ERR_INVALID_INPUT)
            );
            assert_eq!(e.operation(), OP);
            assert_eq!(e.entity(), None);
        }
    }

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
    fn create_corners_notes_the_unsupported_vertex() {
        let mut k = Kernel::default();
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
    }

    #[test]
    fn do_fillet_keeps_the_missing_corner_note_through_the_later_failure() {
        let mut k = Kernel::default();
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
        let result = session.do_fillet(&mut k);
        // The later stage's own failure is returned.
        assert_eq!(
            result.as_ref().err().and_then(SolidError::upstream_status),
            Some(1001),
            "{result:?}"
        );
        let error = session.error_info();
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
    }

    /// A missing-corner note left on the session before `do_fillet` is not this call's:
    /// a later failure of a fillet with no unsupported corner reports exactly the error
    /// info it reports without the stale note. The case's failure is not a no-room
    /// refusal, so the snapshot of the note (not the no-room guard) is what keeps the
    /// stale note out.
    #[test]
    fn do_fillet_does_not_keep_a_stale_missing_corner_note() {
        let run = |stale: bool| {
            let mut k = Kernel::default();
            let mut body =
                crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0)
                    .unwrap();
            let edge = crate::topology::brep::p1::edges(&body, None).unwrap()[0];
            let mut session = FilletSession::new(&mut k, &mut body).unwrap();
            session
                .prepare_constant_radius_edges(&mut k, &[edge], STALE_CASE_RADIUS, 1, 1, 1.0)
                .unwrap();
            if stale {
                session.executive.error.code = FilletErrorType::CornerUnsupported as i32;
                session.executive.error.comment = "a stale note".into();
            }
            let result = session.do_fillet(&mut k);
            assert!(result.is_err(), "stale {stale}: {result:?}");
            let e = session.error_info();
            (e.code, e.comment.clone(), e.no_room, e.vertex)
        };
        let baseline = run(false);
        // the fillet surface computation failure (code 4): neither a no-room refusal nor
        // an unsupported corner
        assert_eq!(baseline.0, 4, "{baseline:?}");
        assert_eq!(baseline.2, None, "{baseline:?}");
        assert_eq!(run(true), baseline);
    }

    /// The radius of [`do_fillet_does_not_keep_a_stale_missing_corner_note`]'s round on one
    /// edge of the unit cube: five times the faces, so the fillet surface fails (code 4,
    /// not a no-room refusal) and the edge has no corner to note.
    const STALE_CASE_RADIUS: f64 = 5.0;

    /// What a fillet of the three edges at the unit cube's corner at the origin, rounded by
    /// 0.2, reports when `spoil` is done to its session once it is prepared: the returned
    /// error of `do_fillet`, the session's error info after it, and the corner's vertex and
    /// point.
    fn spoiled_cube_corner(
        spoil: impl FnOnce(&mut FilletSession<'_>, VertexId),
    ) -> (
        Result<()>,
        ErrorInfo<FilletObject>,
        Option<FilletObject>,
        Option<Point3>,
    ) {
        let mut k = Kernel::default();
        let mut body =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0)
                .unwrap();
        let corner = crate::topology::brep::p1::vertices(&body, None)
            .unwrap()
            .into_iter()
            .find(|v| crate::topology::vertex::point(&body, *v).unwrap() == Point3::ZERO)
            .unwrap();
        let edges = crate::topology::brep::p1::edges(&body, None)
            .unwrap()
            .into_iter()
            .filter(|e| {
                crate::topology::edge::vertices(&body, *e, None)
                    .unwrap()
                    .contains(&Some(corner))
            })
            .collect::<Vec<_>>();
        assert_eq!(edges.len(), 3);
        let at = crate::topology::vertex::point(&body, corner).ok();
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &edges, 0.2, 1, 1, 1.0)
            .unwrap();
        spoil(&mut session, corner);
        let result = session.do_fillet(&mut k);
        let vertex = FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(corner));
        (result, session.error_info().clone(), Some(vertex), at)
    }

    /// [`spoiled_cube_corner`] with the surface of one face at the corner taken away, so
    /// the classification of the corner fails where it tests the convexity of the corner's
    /// edges; with `set_back`, the corner is marked set back, which is no Ball, so the
    /// Ball's classification passes it by and the Setback's is the one that fails.
    fn cube_corner_with_a_face_without_surface(
        set_back: bool,
    ) -> (
        Result<()>,
        ErrorInfo<FilletObject>,
        Option<FilletObject>,
        Option<Point3>,
    ) {
        spoiled_cube_corner(|session, corner| {
            if set_back {
                session.add_set_back_vertex(corner);
            }
            let target = &mut *session.objects.target;
            let face = crate::topology::vertex::faces(target, corner).unwrap()[0];
            target.face_mut(face).unwrap().surface = None;
        })
    }

    /// The error the classification of [`cube_corner_with_a_face_without_surface`] fails
    /// with.
    fn face_without_surface() -> SolidError {
        SolidError::InvalidInput {
            operation: "topology::edgeuse::p3",
            reason: "EvaluateBinormal: face without surface",
        }
    }

    /// The note of the corner whose classification failed, as before (#19803): unsupported,
    /// a failure, the fork's comment, at the corner's vertex and point; and the filleting's
    /// own failure returned, its operation, status and cause unchanged.
    fn assert_noted_as_before(
        (result, error, vertex, at): &(
            Result<()>,
            ErrorInfo<FilletObject>,
            Option<FilletObject>,
            Option<Point3>,
        ),
    ) {
        assert_eq!(error.code, FilletErrorType::CornerUnsupported as i32);
        assert_eq!(error.status, FilStatus::Failure as i32);
        assert_eq!(error.comment, "Corner case not implemented");
        assert_eq!(error.vertex, *vertex);
        assert_eq!(error.location, *at);
        let e = result.as_ref().unwrap_err();
        assert_eq!(e.operation(), "blend::fillet_executive::session::finish");
        assert_eq!(e.upstream_status(), Some(crate::error::SM_ERR), "{e:?}");
        // the stage's error here is not a record that can keep a cause, so the
        // classification's error is not chained onto it: the chain is as it was
        assert_eq!(e.cause(), Some(&face_without_surface()), "{e:?}");
    }

    #[test]
    fn a_failed_ball_classification_is_kept_with_the_unsupported_corner() {
        let run = cube_corner_with_a_face_without_surface(false);
        assert_noted_as_before(&run);
        assert_eq!(
            run.1.unsupported_corner_cause(),
            Some(&face_without_surface())
        );
    }

    #[test]
    fn a_failed_setback_classification_is_kept_with_the_unsupported_corner() {
        let run = cube_corner_with_a_face_without_surface(true);
        assert_noted_as_before(&run);
        assert_eq!(
            run.1.unsupported_corner_cause(),
            Some(&face_without_surface())
        );
    }

    /// Without the spoil the cube's corner is the Ball and the fillet builds: the spoil, not
    /// the case, is what fails the classifications above.
    /// #20216: a corner classification that fails with a cancellation, an unported call or a
    /// certificate out of its budget passes that error on, not an unsupported corner; any other
    /// failure (the face without a surface of #19803) is the unsupported corner keeping it. No
    /// model reaches the arms with the first kind today (no cancellation is polled under the
    /// classifications, and nothing under them makes an unported or not-certified error), so the
    /// errors are given to the arms' function as the classification would return them.
    #[test]
    fn a_classification_cancelled_or_unported_is_passed_on_not_an_unsupported_corner() {
        let mut k = Kernel::default();
        let body =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 1.0, 1.0, 1.0)
                .unwrap();
        let vertex = crate::topology::brep::p1::vertices(&body, None)
            .unwrap()
            .into_iter()
            .find(|v| crate::topology::vertex::point(&body, *v).unwrap() == Point3::ZERO)
            .unwrap();
        let not_certified = SolidError::blend(
            "blend::fillet_geom::tangent_end_rib",
            crate::error::blend::BlendFailure::NotCertified {
                halvings: 3,
                stations: 9,
            },
        );
        for given in [
            SolidError::cancelled("blend::blend_corner::ball"),
            SolidError::unported("evaluate_binormal", "unit"),
            not_certified,
        ] {
            match unclassified_corner::<FilletObject>(&body, vertex, given.clone()) {
                Err(e) => assert_eq!(e, given),
                Ok(made) => panic!("{given:?} became {made:?}"),
            }
        }
        match unclassified_corner::<FilletObject>(&body, vertex, face_without_surface()) {
            Ok(CornerCreation::Unsupported { location, cause }) => {
                assert_eq!(location, Some(Point3::ZERO));
                assert_eq!(cause, Some(face_without_surface()));
            }
            other => panic!("{other:?}"),
        }
    }

    /// Which of the three edges at a corner of [`box_in_torus`] are rounded: the box's upright
    /// edge `x|y` (x = y = 1) and the edges where the box's +x and +y faces meet the torus.
    #[derive(Clone, Copy, Debug, PartialEq)]
    enum TorusCornerEdge {
        XY,
        XTorus,
        YTorus,
    }

    /// What rounding some edges of a corner of [`box_in_torus`] gives.
    #[derive(Debug)]
    enum TorusCornerFillet {
        /// Built: whether the body passes the level-2 check.
        Built(bool),
        /// Refused as too large: its reason and the size it names.
        Refused(
            Option<crate::blend::fillet_executive::NoRoomReason>,
            Option<f64>,
        ),
        /// Failed otherwise.
        Failed(SolidError),
    }

    /// The unit box intersected with the torus about Z through (0, 0, 0.5), radii 1.0 and
    /// 0.6 (#20314): its corners where the +x and +y faces meet the torus are at
    /// (1, 1, 0.0659) and (1, 1, 0.934); the torus face ends on the box's bottom and top.
    fn box_in_torus(k: &mut Kernel) -> Brep {
        let bx =
            crate::primitive::api_primitives::create_box(k, Point3::ZERO, 1.0, 1.0, 1.0).unwrap();
        let torus =
            crate::primitive::api_primitives::create_torus(k, Point3::new(0.0, 0.0, 0.5), 1.0, 0.6)
                .unwrap();
        crate::boolean::api_brep::boolean_intersection(k, bx, torus).unwrap()
    }

    /// The tolerances of the edges where the box's +x and +y faces meet the torus at the corner
    /// of [`box_in_torus`] nearest `near`: a refusal's fit margin is a thousand of the blended
    /// edge's tolerances (`FitMargin`).
    fn torus_corner_edge_tolerances(near: Point3) -> Vec<f64> {
        let mut k = Kernel::default();
        let body = box_in_torus(&mut k);
        let corner = crate::topology::brep::p1::vertices(&body, None)
            .unwrap()
            .into_iter()
            .min_by(|a, b| {
                let d = |v| (crate::topology::vertex::point(&body, v).unwrap() - near).length();
                d(*a).total_cmp(&d(*b))
            })
            .unwrap();
        let mut out = Vec::new();
        for e in crate::topology::brep::p1::edges(&body, None).unwrap() {
            let ends = crate::topology::edge::vertices(&body, e, None).unwrap();
            if !ends.contains(&Some(corner)) {
                continue;
            }
            let other = ends
                .iter()
                .flatten()
                .find(|v| **v != corner)
                .copied()
                .unwrap();
            let o = crate::topology::vertex::point(&body, other).unwrap();
            if (o.x - 1.0).abs() < 1e-9 && (o.y - 1.0).abs() < 1e-9 {
                continue; // the upright edge x|y
            }
            out.push(crate::topology::edge::tolerance(&body, e).unwrap());
        }
        out
    }

    /// Round `which` edges at the corner of [`box_in_torus`] nearest `near` by `radius`.
    fn round_torus_corner(
        near: Point3,
        which: &[TorusCornerEdge],
        radius: f64,
    ) -> TorusCornerFillet {
        let mut k = Kernel::default();
        let mut body = box_in_torus(&mut k);
        let corner = crate::topology::brep::p1::vertices(&body, None)
            .unwrap()
            .into_iter()
            .min_by(|a, b| {
                let d = |v| (crate::topology::vertex::point(&body, v).unwrap() - near).length();
                d(*a).total_cmp(&d(*b))
            })
            .unwrap();
        let at = crate::topology::vertex::point(&body, corner).unwrap();
        assert!((at - near).length() < 1e-3, "{at:?}");
        let mut edges = Vec::new();
        for e in crate::topology::brep::p1::edges(&body, None).unwrap() {
            let ends = crate::topology::edge::vertices(&body, e, None).unwrap();
            if !ends.contains(&Some(corner)) {
                continue;
            }
            let other = ends
                .iter()
                .flatten()
                .find(|v| **v != corner)
                .copied()
                .unwrap();
            let o = crate::topology::vertex::point(&body, other).unwrap();
            let kind = if (o.x - 1.0).abs() < 1e-9 && (o.y - 1.0).abs() < 1e-9 {
                TorusCornerEdge::XY
            } else if (o.x - 1.0).abs() < 1e-9 {
                TorusCornerEdge::XTorus
            } else {
                TorusCornerEdge::YTorus
            };
            if which.contains(&kind) {
                edges.push(e);
            }
        }
        assert_eq!(edges.len(), which.len(), "{which:?} at {at:?}");
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &edges, radius, 1, 1, 1.0)
            .unwrap();
        match session.do_fillet(&mut k) {
            Ok(()) => {
                drop(session);
                let check = crate::topology::brep::validation::check_brep(
                    &mut k,
                    &mut body,
                    crate::topology::brep::validation::CheckLevel::Level2,
                )
                .unwrap();
                TorusCornerFillet::Built(check.lines().is_empty())
            }
            Err(e) => match session.error_info().no_room_reason() {
                Some(reason) => {
                    TorusCornerFillet::Refused(Some(reason), session.fit_size().map(|f| f.size))
                }
                None => TorusCornerFillet::Failed(e),
            },
        }
    }

    /// #20314: two rounds whose rails leave the torus face across its rim (the box's bottom or
    /// top) meet at a corner of the box cut by a torus. From r* = 0.0958289 (closed form: the
    /// ball touching the +x and +y faces and the torus has its contact on the torus at the rim)
    /// both rails overflow; the overflow is not built where two overflowing rounds meet, so
    /// the blend is refused as leaving its face, naming r* less the fit margin. It failed in
    /// the rails' trim (a reversed interval) and at 0.0958 returned an invalid body. One
    /// overflowing round, alone or with the upright edge, still builds.
    #[test]
    fn two_overflowing_rounds_meeting_at_a_corner_are_refused_naming_where_the_corner_leaves_the_face(
    ) {
        use TorusCornerEdge::{XTorus, YTorus, XY};
        let r_star = 0.095_828_866_164_137_94;
        let low = Point3::new(1.0, 1.0, 0.0659);
        let high = Point3::new(1.0, 1.0, 0.934);
        let refused_or_built =
            |near: Point3, which: &[TorusCornerEdge], radius: f64| -> Option<f64> {
                match round_torus_corner(near, which, radius) {
                    TorusCornerFillet::Built(valid) => {
                        assert!(valid, "{which:?} at {near:?} r {radius}: built but invalid");
                        None
                    }
                    TorusCornerFillet::Refused(reason, fit) => {
                        assert_eq!(
                            reason,
                            Some(crate::blend::fillet_executive::NoRoomReason::RailLeavesFace),
                            "{which:?} at {near:?} r {radius}"
                        );
                        let fit = fit.unwrap_or_else(|| {
                            panic!("{which:?} at {near:?} r {radius}: refused naming no size")
                        });
                        // the bound is open: r* less the fit margin of the edge it is noted on,
                        // to the bisection's and the balls' solve accuracy (1e-6)
                        let margins = torus_corner_edge_tolerances(near);
                        assert!(
                            margins
                                .iter()
                                .any(|tol| (fit - (r_star - 1000.0 * tol)).abs() < 1e-6),
                            "{which:?} at {near:?} r {radius}: fit {fit}, r* {r_star}"
                        );
                        Some(fit)
                    }
                    TorusCornerFillet::Failed(e) => panic!("{which:?} at {near:?} r {radius}: {e}"),
                }
            };
        for near in [low, high] {
            // the x|torus + y|torus pair alone is another event: its rails are judged to fit
            // their faces (no overflow), and the corner where the two rounds meet leaves the
            // torus face; not this refusal's (#20359)
            let which = &[XY, XTorus, YTorus][..];
            if let Some(fit) = refused_or_built(near, which, 0.1) {
                assert!(
                    matches!(
                        round_torus_corner(near, which, fit),
                        TorusCornerFillet::Built(true)
                    ),
                    "{which:?} at {near:?} at its fit {fit}"
                );
            }
            refused_or_built(near, which, 0.0959);
            for radius in [0.09, 0.095] {
                assert!(
                    matches!(
                        round_torus_corner(near, &[XY, XTorus, YTorus], radius),
                        TorusCornerFillet::Built(true)
                    ),
                    "{near:?} r {radius}"
                );
            }
            refused_or_built(near, &[XY, XTorus, YTorus], 0.0958);
            for which in [&[XTorus][..], &[XTorus, XY][..]] {
                assert!(
                    matches!(
                        round_torus_corner(near, which, 0.1),
                        TorusCornerFillet::Built(true)
                    ),
                    "{which:?} at {near:?} r 0.1"
                );
            }
        }
    }

    #[test]
    fn the_unspoiled_cube_corner_builds() {
        let (result, error, _, _) = spoiled_cube_corner(|_, _| {});
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(error.unsupported_corner_cause(), None);
    }

    /// The pentagonal pyramid with three of its five apex edges rounded by 0.5:
    /// `do_fillet`'s result and the error info after it.
    fn pyramid_apex() -> (Result<()>, ErrorInfo<FilletObject>) {
        let mut k = Kernel::default();
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
                    .contains(&Some(apex))
            })
            .collect::<Vec<_>>();
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &side_edges[..3], 0.5, 1, 1, 1.0)
            .unwrap();
        let result = session.do_fillet(&mut k);
        (result, session.error_info().clone())
    }

    /// #19803: the failure a missing corner produces is the one `do_fillet`'s chain keeps a
    /// failed classification's error under (`finish::is_missing_corner`): the pyramid's apex,
    /// unsupported by its kind, gives it as the root of the later failure. Its corner made
    /// no classification, so nothing is kept under it and the chain is as before.
    #[test]
    fn the_missing_corner_is_the_root_a_classification_error_goes_under() {
        let (result, error) = pyramid_apex();
        let e = result.unwrap_err();
        assert!(finish::is_missing_corner(e.root_cause()), "{e:?}");
        assert_eq!(error.code, FilletErrorType::CornerUnsupported as i32);
        assert_eq!(error.unsupported_corner_cause(), None);
    }

    #[test]
    fn a_no_room_refusal_after_an_unsupported_corner_names_no_stale_site() {
        let mut k = Kernel::default();
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
        // step by step: the apex corner is noted unsupported, at the apex
        session.create_corners(&k, None).unwrap();
        let site = session.error_site();
        assert_eq!(
            (site.vertex.map(crate::error::TargetVertex::id), site.at),
            (Some(apex), Some(apex_point))
        );
        // a later no-room refusal noted without a site does not name the apex
        assert!(session
            .executive
            .record_no_room(
                crate::blend::fillet_executive::NoRoomReason::RailLeavesFace,
                "no room"
            )
            .is_err());
        assert_eq!(session.error_info().code, FilletErrorType::NoRoom as i32);
        // (the record itself keeps the earlier fields, as upstream's overload does)
        assert_eq!(session.error_info().location, Some(apex_point));
        assert_eq!(session.error_site(), crate::error::Site::default());
        // a no-room refusal noted at a vertex names it again
        session.executive.error.note_vertex_error(
            Some(FilletObject::Topology(
                FilletBody::Target,
                EntityRef::Vertex(apex),
            )),
            Some(apex_point),
            FilStatus::Failure,
            FilletErrorType::NoRoom,
            Some("no room at the corner"),
        );
        let site = session.error_site();
        assert_eq!(
            (site.vertex.map(crate::error::TargetVertex::id), site.at),
            (Some(apex), Some(apex_point))
        );
    }

    /// The band check's search for a point's foot on a band's run (#19243). Every way the
    /// foot on one edge can fail (a drop that fails, a drop that finds no point, an
    /// evaluation that fails), before, between or after edges whose feet are found, returns
    /// the failure for a sample on its face and drops a sample off it; no other edge's foot
    /// stands in for it. With every foot found, the nearest is kept and the sample is not
    /// classified.
    #[test]
    fn a_foot_that_cannot_be_found_on_the_run_is_not_replaced() {
        use crate::curve::curve::p6::DroppedPoint;
        use std::cell::Cell;
        /// How a test edge's foot comes out.
        #[derive(Debug, Clone, Copy, PartialEq)]
        enum Edge {
            /// Its foot is found, here.
            At(Point3),
            /// Its drop fails.
            DropFails,
            /// Its drop finds no point.
            NoPoint,
            /// Its drop finds a point whose evaluation fails.
            EvaluationFails,
        }
        fn failure() -> SolidError {
            SolidError::invalid_input("test", "injected failure")
        }
        let point = Point3::new(0.0, 0.0, 0.0);
        let near = Point3::new(1.0, 0.0, 0.0);
        let far = Point3::new(0.0, 5.0, 0.0);
        let classified = Cell::new(0);
        let search = |run: &[Edge], off_face: bool| {
            let foot = foot_on_run(
                point,
                run.iter().map(|&edge| Ok(edge)),
                |&edge, _| match edge {
                    Edge::DropFails => Err(failure()),
                    Edge::NoPoint => Ok(DroppedPoint::default()),
                    Edge::At(_) | Edge::EvaluationFails => Ok(DroppedPoint {
                        success: true,
                        parameter: 0.5,
                        distance: 0.0,
                    }),
                },
                |&edge, _| match edge {
                    Edge::At(q) => Ok(q),
                    _ => Err(failure()),
                },
                || {
                    classified.set(classified.get() + 1);
                    Ok(off_face)
                },
            );
            foot.map(|f| match f {
                FootOnRun::Found { edge, point, .. } => Some((edge, point)),
                FootOnRun::SampleOffFace => None,
            })
        };
        // every foot found: the nearest, and the sample is not classified
        let found = search(&[Edge::At(far), Edge::At(near)], false).unwrap();
        assert_eq!(found, Some((Edge::At(near), near)));
        assert_eq!(classified.get(), 0);
        // a run without edges is a defect, not a point
        assert_eq!(
            search(&[], false).unwrap_err().kind(),
            band_defect(RUN_EDGE_GATHERED).kind()
        );
        for failing in [Edge::DropFails, Edge::NoPoint, Edge::EvaluationFails] {
            for run in [
                vec![failing, Edge::At(far)],
                vec![Edge::At(far), failing, Edge::At(near)],
                vec![Edge::At(near), failing],
                vec![failing],
            ] {
                classified.set(0);
                let on_face = search(&run, false);
                let kind = if failing == Edge::NoPoint {
                    band_defect(RUN_EDGE_GATHERED).kind()
                } else {
                    failure().kind()
                };
                assert_eq!(
                    on_face.as_ref().map_err(SolidError::kind),
                    Err(kind),
                    "a sample on its face gets the failure"
                );
                assert_eq!(
                    search(&run, true).unwrap(),
                    None,
                    "a sample off its face is dropped"
                );
                assert_eq!(
                    classified.get(),
                    2,
                    "the sample is classified once per search"
                );
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
