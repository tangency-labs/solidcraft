//! Arena-based B-rep topology with typed ids.
//!
//! Upstream: `source/SMLib/inc/SmBrep.h`, `SmRegion.h`, `SmShell.h`, `SmFace.h`,
//! `SmLoop.h`, `SmEdge.h`, `SmVertex.h`, the use records `SmFaceuse.h`, `SmLoopuse.h`,
//! `SmEdgeuse.h`, `SmVertexuse.h`, and `SmTopology.h` / `SmOwningTopology.h`.
//!
//! | Rust                        | Upstream                                              |
//! |-----------------------------|-------------------------------------------------------|
//! | [`Brep`]                    | `SmBrep` (face/edge/vertex lists `m_p*ListHead`, `m_bValidateOnly`; its own members in [`brep::p1`]) |
//! | [`Region`] / [`RegionId`]   | `SmRegion` (minimal: its list owner)                  |
//! | [`Shell`] / [`ShellId`]     | `SmShell`                                             |
//! | [`Faceuse`] / [`FaceuseId`] | `SmFaceuse` (unit `topology.edgeuse_p1`)              |
//! | [`Face`] / [`FaceId`]       | `SmFace` (unit `topology.face_p2`)                    |
//! | [`Loopuse`] / [`LoopuseId`] | `SmLoopuse` (unit `topology.loopuse`)                 |
//! | [`Loop`] / [`LoopId`]       | `SmLoop`                                              |
//! | [`Edgeuse`] / [`EdgeuseId`] | `SmEdgeuse` (unit `topology.edgeuse_p1`)              |
//! | [`Edge`] / [`EdgeId`]       | `SmEdge`                                              |
//! | [`Vertexuse`] / [`VertexuseId`] | `SmVertexuse` (unit `topology.vertexuse`)         |
//! | [`Vertex`] / [`VertexId`]   | `SmVertex` (unit `topology.vertex`)                   |
//! | [`ObjectHandle`]            | an `SmObject*` a record keeps but never dereferences  |
//! | [`Arena`]                   | upstream's intrusive `SmListNode` linked lists        |
//!
//! Upstream links entities with raw pointers in intrusive lists; the port stores each
//! kind in a flat [`Arena`] owned by the [`Brep`] and links them with typed ids, so
//! iteration order is insertion order (deterministic) and there is no shared
//! ownership. This file is the **skeleton** shared by every unit: the record fields
//! below are the minimum the scaffolding needs. The topology units in
//! `plan/modules.toml` (regions, face/loop/edge/vertex uses, geometry binding,
//! tolerances, attributes) extend these records; keep the id/arena/accessor shape.
//!
//! The use records and the fields marked "CO-PORTED" were added by unit
//! `topology.edgeuse_p1` (the minimum its functions read). An owner's circular
//! `m_pList` / `m_pNext` / `m_pLast` list is a `Vec` of ids in list order from the
//! head (`m_pList`); an object's `m_pNext` / `m_pLast` are its neighbours in that
//! `Vec` (cyclically). The low-level `add_*_record` and `*_mut` accessors are the
//! friend writes of `SmBrepConstructor`: they link records without validation (a
//! B-rep graph is cyclic, so a record may name ids added later).

pub mod brep;
pub mod brep_cache;
pub mod brep_data;
pub mod database_io;
pub mod edge;
pub mod edgeuse;
pub mod face;
pub mod loopuse;
pub mod proto_topology;
pub mod shell;
#[allow(clippy::module_inception)]
pub mod topology;
pub mod topology_traverser;
pub mod trimming_tools;
pub mod vertex;
pub mod vertexuse;

use core::marker::PhantomData;

use crate::curve::curve::Curve;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::attribute::ObjectKey;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::geom::tol::{USE_DEFAULT, ZONE_TOL_3D};
use crate::geom::vector2d::Point2;
use crate::heal::gap::{EdgeEdgeGap, EdgeFaceGap, VertexEdgeGap, VertexFaceGap};
use crate::math::{Point3, BIG_DOUBLE};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::poly::PolyVertexId;
use crate::surface::surface::p2::DropCurveFail;
use crate::surface::surface::Surface;

/// Implemented by every typed id: conversion to and from a dense arena index.
pub trait ArenaId: Copy + Eq + core::fmt::Debug {
    /// Builds an id from an arena index.
    fn from_index(index: u32) -> Self;
    /// The arena index of this id.
    fn index(self) -> u32;
    /// This id as an error-context [`EntityRef`].
    fn entity(self) -> EntityRef;
}

macro_rules! typed_id {
    ($(#[$doc:meta])* $name:ident => $variant:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u32);

        impl $name {
            /// Builds an id from a raw index. Ids are only meaningful for the B-rep
            /// (or collection) that issued them.
            #[must_use]
            pub const fn from_index(index: u32) -> Self {
                Self(index)
            }

            /// The raw index of this id.
            #[must_use]
            pub const fn index(self) -> u32 {
                self.0
            }
        }

        impl ArenaId for $name {
            fn from_index(index: u32) -> Self {
                Self(index)
            }
            fn index(self) -> u32 {
                self.0
            }
            fn entity(self) -> EntityRef {
                EntityRef::$variant(self)
            }
        }
    };
}

typed_id!(
    /// Identifies a [`Brep`] within a caller-owned collection of B-reps.
    BrepId => Brep
);
typed_id!(
    /// Identifies a [`Shell`] within its [`Brep`].
    ShellId => Shell
);
typed_id!(
    /// Identifies a [`Face`] within its [`Brep`].
    FaceId => Face
);
typed_id!(
    /// Identifies a [`Loop`] within its [`Brep`].
    LoopId => Loop
);
typed_id!(
    /// Identifies an [`Edge`] within its [`Brep`].
    EdgeId => Edge
);
typed_id!(
    /// Identifies a [`Vertex`] within its [`Brep`].
    VertexId => Vertex
);
typed_id!(
    /// Identifies a [`Region`] within its [`Brep`].
    RegionId => Region
);
typed_id!(
    /// Identifies a [`Faceuse`] within its [`Brep`].
    FaceuseId => Faceuse
);
typed_id!(
    /// Identifies a [`Loopuse`] within its [`Brep`].
    LoopuseId => Loopuse
);
typed_id!(
    /// Identifies an [`Edgeuse`] within its [`Brep`].
    EdgeuseId => Edgeuse
);
typed_id!(
    /// Identifies a [`Vertexuse`] within its [`Brep`].
    VertexuseId => Vertexuse
);

/// A dense, append-only store of `T` addressed by typed id `I`.
#[derive(Debug, Clone, PartialEq)]
pub struct Arena<I, T> {
    items: Vec<T>,
    _id: PhantomData<fn() -> I>,
}

impl<I, T> Default for Arena<I, T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            _id: PhantomData,
        }
    }
}

impl<I: ArenaId, T> Arena<I, T> {
    /// Appends an item and returns its id.
    ///
    /// Fails with [`SolidError::InvalidInput`] if the arena already holds `u32::MAX`
    /// items.
    pub fn insert(&mut self, item: T) -> Result<I> {
        let index = u32::try_from(self.items.len()).map_err(|_| SolidError::InvalidInput {
            operation: "topology::Arena::insert",
            reason: "arena is full",
        })?;
        self.items.push(item);
        Ok(I::from_index(index))
    }

    /// The item with id `id`, or `None` if the id is out of range.
    #[must_use]
    pub fn get(&self, id: I) -> Option<&T> {
        self.items.get(id.index() as usize)
    }

    /// Mutable access to the item with id `id`.
    #[must_use]
    pub fn get_mut(&mut self, id: I) -> Option<&mut T> {
        self.items.get_mut(id.index() as usize)
    }

    /// Number of items.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True when the arena is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterates `(id, item)` in insertion order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (I, &T)> + '_ {
        // Arena length never exceeds u32::MAX (checked in `insert`), so the cast is
        // lossless.
        self.items
            .iter()
            .enumerate()
            .map(|(i, t)| (I::from_index(i as u32), t))
    }

    /// Iterates ids in insertion order.
    pub fn ids(&self) -> impl ExactSizeIterator<Item = I> + '_ {
        (0..self.items.len()).map(|i| I::from_index(i as u32))
    }
}

/// An `SmObject*` a topology record keeps but never dereferences (the surface,
/// curve and B-rep pointers of an `SmDropCurveFail`): the object's kind and the
/// identity it is attributed and notified under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectHandle {
    /// What the object is.
    pub kind: ObjectKind,
    /// Its identity.
    pub key: ObjectKey,
}

impl SolutionObject for ObjectHandle {
    fn kind(&self) -> ObjectKind {
        self.kind
    }
}

/// A vertex (`SmVertex`): a point where edges meet (the methods are in unit
/// `topology.vertex`, [`vertex`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    /// `m_vPoint`: position in model space.
    pub point: Point3,
    /// `m_sZoneTol3d` (`SM_USE_OLDTOL`): the vertex tolerance.
    pub tolerance: f64,
    /// `m_pList` / `m_lListSize`: the vertex's vertex uses in list order from the head
    /// (the primary vertex use, `GetPrimaryVertexuse`, is first).
    pub vertexuses: Vec<VertexuseId>,
    /// `m_pPolyVertex3D`: the temporary back pointer to the 3D poly vertex made from the
    /// vertex during tessellation.
    pub poly_vertex: Option<PolyVertexId>,
}

impl Default for Vertex {
    /// `SmVertex()`: at the origin, tolerance `0`, no vertex uses, no poly vertex.
    fn default() -> Self {
        Self {
            point: Point3::new(0.0, 0.0, 0.0),
            tolerance: 0.0,
            vertexuses: Vec::new(),
            poly_vertex: None,
        }
    }
}

/// An edge (`SmEdge`): a bounded curve between two vertices.
#[derive(Debug, Clone)]
pub struct Edge {
    /// Start and end vertex (equal for a closed edge).
    pub vertices: [VertexId; 2],
    /// `m_pList` (CO-PORTED): the edge's edge uses in radial list order; the first is
    /// the primary edge use (`GetPrimaryEdgeuse`).
    pub edgeuses: Vec<EdgeuseId>,
    /// `m_vInterval` (CO-PORTED): the used parameter interval of the curve.
    pub interval: Extent1d,
    /// `m_sZoneTol3d` (CO-PORTED, `SM_USE_OLDTOL`): the edge tolerance.
    pub tolerance: f64,
    /// `m_pCurve` (CO-PORTED): the edge's 3D curve.
    pub curve: Option<Curve>,
}

/// One oriented use of an edge in a loop (upstream `SmEdgeuse`, simplified).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopEdge {
    /// The edge used.
    pub edge: EdgeId,
    /// True when the loop traverses the edge from `vertices[0]` to `vertices[1]`.
    pub forward: bool,
}

/// A closed boundary loop of a face (`SmLoop`).
#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    /// Face this loop bounds.
    pub face: FaceId,
    /// Edges in traversal order.
    pub edges: Vec<LoopEdge>,
    /// `m_pLU` (CO-PORTED): the primary loop use (`GetLoopuse`).
    pub loopuse: Option<LoopuseId>,
}

/// A face (`SmFace`): a bounded region of a surface. The methods are in unit
/// `topology.face_p2` ([`face::p2`]).
#[derive(Debug, Clone)]
pub struct Face {
    /// Shell containing this face.
    pub shell: ShellId,
    /// Boundary loops; upstream keeps the outer loop first.
    pub loops: Vec<LoopId>,
    /// `m_pSurface`: the face's surface (`GetSurface`).
    pub surface: Option<Surface>,
    /// `m_pFU`: the primary (upward, [`OrientType::Same`]) face use
    /// (`GetUpwardFaceuse`).
    pub faceuse: Option<FaceuseId>,
    /// `m_vUVDomain`: the domain of the surface the face uses (`GetUVDomain`).
    pub uv_domain: Extent2d,
    /// `m_bRectangularTrim` (an `SmBoolean`: `1` the face has one rectangular outer
    /// loop of four iso trim curves spanning [`Face::uv_domain`], `0` it has not,
    /// [`face::p2::UNSURE`] not yet known).
    pub rectangular_trim: i32,
    /// `m_sZoneTol3d` (`SM_USE_OLDTOL`): the face tolerance (`GetTolerance`).
    pub tolerance: f64,
}

impl Face {
    /// `SmFace()` in `shell`: no loops, surface or face use, the "inside out" UV
    /// domain, rectangular trim [`face::p2::UNSURE`] and tolerance `0`.
    #[must_use]
    pub const fn new(shell: ShellId) -> Self {
        Self {
            shell,
            loops: Vec::new(),
            surface: None,
            faceuse: None,
            uv_domain: Extent2d::new(),
            rectangular_trim: face::p2::UNSURE,
            tolerance: 0.0,
        }
    }
}

/// A shell (`SmShell`): a connected set of faces.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shell {
    /// Faces of the shell, in insertion order.
    pub faces: Vec<FaceId>,
    /// `m_pListOwner` (CO-PORTED): the region owning the shell (`GetRegion`).
    pub region: Option<RegionId>,
    /// Native `m_tShellType`, independent of the list head during construction.
    pub shell_type: shell::ShellType,
    /// Native list members in head-first circular traversal order.
    pub members: shell::ShellMembers,
}

/// A region (`SmRegion`, CO-PORTED minimal record).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Region {
    /// `m_pListOwner != NULL`: the region is on the B-rep's region list, so
    /// `GetBrep` reaches the B-rep.
    pub in_brep: bool,
    /// Native region shell list, in traversal order from its head.
    pub shells: Vec<ShellId>,
    /// Native `SmBoolean` `m_bIsVoidFlag` (0 at construction; 2 is `UNSURE`).
    pub is_void: i32,
}

/// One use of a face (`SmFaceuse`): the face seen from one side.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Faceuse {
    /// `m_pListOwner`: the shell owning the face use (`GetShell`).
    pub shell: Option<ShellId>,
    /// `m_pF`: the face used (`GetFace`).
    pub face: Option<FaceId>,
    /// `m_pFUMate`: the face's other face use (`GetMate`).
    pub mate: Option<FaceuseId>,
    /// `m_eOrientation`: [`OrientType::Same`] when the face use normal is the
    /// surface normal.
    pub orientation: OrientType,
    /// `m_pList`: the loop uses, outer loop use first.
    pub loopuses: Vec<LoopuseId>,
    /// Literal sibling links when present, even if the owner has no list head.
    /// `None` uses the owner's canonical face-use vector during construction.
    pub sibling_links: Option<shell::FaceuseSiblingLinks>,
}

/// What a loop use starts from (`m_tLoopuseType` and `m_pEUorVU`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopuseStart {
    /// `SmUnknown_TYPE` (a loop use not yet built).
    #[default]
    Unknown,
    /// `SmEdgeuse_TYPE`: the loop's start edge use.
    Edgeuse(Option<EdgeuseId>),
    /// `SmVertexuse_TYPE`: the single vertex use of a vertex loop.
    Vertexuse(Option<VertexuseId>),
}

/// One use of a loop (`SmLoopuse`; `Default` is `SmLoopuse()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Loopuse {
    /// `m_pListOwner` as an `SmFaceuse` (`GetFaceuse`).
    pub faceuse: Option<FaceuseId>,
    /// `m_pL`: the loop used (`GetLoop`).
    pub loop_: Option<LoopId>,
    /// `m_pLUMate`: the loop use on the face's other side (`GetOtherLoopuse`).
    pub mate: Option<LoopuseId>,
    /// `m_tLoopuseType` and `m_pEUorVU`.
    pub start: LoopuseStart,
    /// `m_eOrientation` (unit `topology.loopuse`): [`OrientType::Same`] for an outer
    /// loop use, [`OrientType::Opposite`] for an inner one.
    pub orientation: OrientType,
}

/// What an edge use connects to (`m_tEdgeuseType` and `m_pSorLU`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EdgeuseOwner {
    /// `SmUnknown_TYPE` (the constructor's value).
    #[default]
    Unknown,
    /// `SmLoopuse_TYPE`: an edge in a face, connected through a loop use.
    Loopuse(Option<LoopuseId>),
    /// `SmShell_TYPE`: a wire edge, connected to a region through a shell.
    Shell(Option<ShellId>),
}

/// One use of an edge (`SmEdgeuse`): the edge seen from one face side or one wire
/// end.
#[derive(Debug, Clone)]
pub struct Edgeuse {
    /// `m_pListOwner`: the edge used (`GetEdge`).
    pub edge: Option<EdgeId>,
    /// `m_tEdgeuseType` and `m_pSorLU`.
    pub owner: EdgeuseOwner,
    /// `m_pCCW`: the next edge use counter-clockwise in the loop.
    pub ccw: Option<EdgeuseId>,
    /// `m_pCW`: the next edge use clockwise in the loop.
    pub cw: Option<EdgeuseId>,
    /// `m_pVU`: the vertex use at the edge use's start.
    pub vertexuse: Option<VertexuseId>,
    /// `m_eOrientation`: [`OrientType::Same`] when the edge use runs with its edge's
    /// curve.
    pub orientation: OrientType,
    /// `m_pUVTrimCurve`: the 2D trim curve, kept on the [`OrientType::Same`] member
    /// of a mated pair (the edge use is its owner). Upstream's `SmBSplineCurve*`: a
    /// B-spline curve or one of its derived kinds (`SmLine`, ...).
    pub uv_trim_curve: Option<Curve>,
    /// `m_sMaxEdgeFaceGap3d`: the cached largest edge/face gap.
    pub max_edge_face_gap: EdgeFaceGap,
    /// `m_sEdgeCCWEdgeGap3d`: the cached gap to the counter-clockwise edge use.
    pub edge_ccw_edge_gap: EdgeEdgeGap,
    /// `m_pDropCurveFail`: what the last `CreateUVTrimCurve` recorded.
    pub drop_curve_fail: Option<Box<DropCurveFail<ObjectHandle>>>,
}

impl Default for Edgeuse {
    /// `SmEdgeuse()`: unlinked, unknown type and orientation, gaps reset.
    fn default() -> Self {
        Self {
            edge: None,
            owner: EdgeuseOwner::Unknown,
            ccw: None,
            cw: None,
            vertexuse: None,
            orientation: OrientType::Unknown,
            uv_trim_curve: None,
            max_edge_face_gap: EdgeFaceGap::new(),
            edge_ccw_edge_gap: EdgeEdgeGap::new(),
            drop_curve_fail: None,
        }
    }
}

/// What a vertex use connects to (`m_tVertexuseType` and `m_pSorLUorEU`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VertexuseOwner {
    /// `SmUnknown_TYPE`.
    #[default]
    Unknown,
    /// `SmEdgeuse_TYPE`: the vertex at an edge use's start.
    Edgeuse(Option<EdgeuseId>),
    /// `SmLoopuse_TYPE`: the vertex of a vertex loop.
    Loopuse(Option<LoopuseId>),
    /// `SmShell_TYPE`: a vertex alone in a region.
    Shell(Option<ShellId>),
}

/// One use of a vertex (`SmVertexuse`; the methods are in unit `topology.vertexuse`,
/// [`vertexuse`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertexuse {
    /// `m_pListOwner`: the vertex used (`GetVertex`).
    pub vertex: Option<VertexId>,
    /// `m_tVertexuseType` and `m_pSorLUorEU`.
    pub owner: VertexuseOwner,
    /// `m_sVertexEdgeGap3d`: the cached vertex/edge gap.
    pub vertex_edge_gap: VertexEdgeGap,
    /// `m_sVertexFaceGap3d`: the cached vertex/face gap.
    pub vertex_face_gap: VertexFaceGap,
    /// `m_sUVPoint` (`GetUVPoint` / `SetUVPoint`; `(SM_BIG_DOUBLE, SM_BIG_DOUBLE)`
    /// until set).
    pub uv_point: Point2,
}

impl Default for Vertexuse {
    /// `SmVertexuse()`: unlinked, unknown type, gaps reset, UV point
    /// `(SM_BIG_DOUBLE, SM_BIG_DOUBLE)`.
    fn default() -> Self {
        Self {
            vertex: None,
            owner: VertexuseOwner::Unknown,
            vertex_edge_gap: VertexEdgeGap::new(),
            vertex_face_gap: VertexFaceGap::new(),
            uv_point: Point2::new(BIG_DOUBLE, BIG_DOUBLE),
        }
    }
}

/// A boundary representation (`SmBrep`): arenas of shells, faces, loops, edges and
/// vertices linked by typed ids.
#[derive(Debug, Clone)]
pub struct Brep {
    shells: Arena<ShellId, Shell>,
    faces: Arena<FaceId, Face>,
    loops: Arena<LoopId, Loop>,
    edges: Arena<EdgeId, Edge>,
    vertices: Arena<VertexId, Vertex>,
    regions: Arena<RegionId, Region>,
    infinite_region: Option<RegionId>,
    faceuses: Arena<FaceuseId, Faceuse>,
    loopuses: Arena<LoopuseId, Loopuse>,
    edgeuses: Arena<EdgeuseId, Edgeuse>,
    vertexuses: Arena<VertexuseId, Vertexuse>,
    /// `m_bValidateOnly`.
    validate_only: bool,
    /// `m_sThisZoneTol3d` (CO-PORTED by `topology.edgeuse_p2`): the B-rep's tolerance.
    tolerance: f64,
    /// The identity the B-rep is attributed and notified under (`SmBrep*`).
    key: ObjectKey,
    /// `SmOwningTopology::m_pList` of the B-rep (unit `topology.brep_p1`): the head of
    /// its circular region list, `None` for the first region record (the region arena
    /// holds the list in traversal order; the head only rotates it).
    region_list_head: Option<RegionId>,
    /// `m_pFaceListHead` (unit `topology.brep_p1`): the face list in its list order
    /// (every face record, once; an importer may reorder it). Upstream appends on
    /// `PostInsert` and unlinks on `Remove`: a record deletion (`DeleteFace` /
    /// `DeleteEdge` / `DeleteVertex`, not ported yet) must remove the id from this
    /// list and its siblings below, as `add_*_record` appends it.
    face_list: Vec<FaceId>,
    /// `m_pEdgeListHead` (unit `topology.brep_p1`): the edge list in its list order.
    edge_list: Vec<EdgeId>,
    /// `m_pVertexListHead` (unit `topology.brep_p1`): the vertex list in its list order.
    vertex_list: Vec<VertexId>,
    /// `m_bEditingEnabled` (unit `topology.brep_p1`): the B-rep is being edited, so
    /// trimmed-surface caches skip the trimmed face data.
    editing_enabled: bool,
    /// `m_dThisModelSizeEstimate` (unit `topology.brep_p1`).
    model_size_estimate: f64,
    /// `m_dThisLargeSmallSizeRatio` (unit `topology.brep_p1`).
    large_small_size_ratio: f64,
    /// `m_pDeletedSurfaces` (unit `topology.brep_p1`): surfaces removed from the B-rep
    /// that are kept until the operation ends because they may still be referenced.
    deleted_surfaces: Vec<Surface>,
}

impl Default for Brep {
    fn default() -> Self {
        Self {
            shells: Arena::default(),
            faces: Arena::default(),
            loops: Arena::default(),
            edges: Arena::default(),
            vertices: Arena::default(),
            regions: Arena::default(),
            infinite_region: None,
            faceuses: Arena::default(),
            loopuses: Arena::default(),
            edgeuses: Arena::default(),
            vertexuses: Arena::default(),
            validate_only: false,
            tolerance: ZONE_TOL_3D,
            key: ObjectKey(0),
            region_list_head: None,
            face_list: Vec::new(),
            edge_list: Vec::new(),
            vertex_list: Vec::new(),
            editing_enabled: false,
            model_size_estimate: USE_DEFAULT,
            large_small_size_ratio: USE_DEFAULT,
            deleted_surfaces: Vec::new(),
        }
    }
}

fn not_found<I: ArenaId>(operation: &'static str, id: I) -> SolidError {
    SolidError::NotFound {
        operation,
        entity: id.entity(),
    }
}

impl Brep {
    /// An empty B-rep.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The region designated as infinite by the B-rep constructor.
    #[must_use]
    pub const fn infinite_region(&self) -> Option<RegionId> {
        self.infinite_region
    }

    /// Friend write of the B-rep's infinite-region identity. Like other record
    /// writes, this permits references to records added later.
    pub fn set_infinite_region_record(&mut self, region: Option<RegionId>) {
        self.infinite_region = region;
    }

    /// `m_bEditingEnabled`.
    #[must_use]
    pub const fn editing_enabled(&self) -> bool {
        self.editing_enabled
    }

    /// Sets `m_bEditingEnabled`.
    pub fn set_editing_enabled(&mut self, enabled: bool) {
        self.editing_enabled = enabled;
    }

    /// All shells.
    #[must_use]
    pub fn shells(&self) -> &Arena<ShellId, Shell> {
        &self.shells
    }

    /// All faces.
    #[must_use]
    pub fn faces(&self) -> &Arena<FaceId, Face> {
        &self.faces
    }

    /// All loops.
    #[must_use]
    pub fn loops(&self) -> &Arena<LoopId, Loop> {
        &self.loops
    }

    /// All edges.
    #[must_use]
    pub fn edges(&self) -> &Arena<EdgeId, Edge> {
        &self.edges
    }

    /// All vertices.
    #[must_use]
    pub fn vertices(&self) -> &Arena<VertexId, Vertex> {
        &self.vertices
    }

    /// The shell `id`.
    pub fn shell(&self, id: ShellId) -> Result<&Shell> {
        self.shells
            .get(id)
            .ok_or_else(|| not_found("topology::Brep::shell", id))
    }

    /// The face `id`.
    pub fn face(&self, id: FaceId) -> Result<&Face> {
        self.faces
            .get(id)
            .ok_or_else(|| not_found("topology::Brep::face", id))
    }

    /// The loop `id`.
    pub fn loop_(&self, id: LoopId) -> Result<&Loop> {
        self.loops
            .get(id)
            .ok_or_else(|| not_found("topology::Brep::loop_", id))
    }

    /// The edge `id`.
    pub fn edge(&self, id: EdgeId) -> Result<&Edge> {
        self.edges
            .get(id)
            .ok_or_else(|| not_found("topology::Brep::edge", id))
    }

    /// The vertex `id`.
    pub fn vertex(&self, id: VertexId) -> Result<&Vertex> {
        self.vertices
            .get(id)
            .ok_or_else(|| not_found("topology::Brep::vertex", id))
    }

    /// All regions.
    #[must_use]
    pub fn regions(&self) -> &Arena<RegionId, Region> {
        &self.regions
    }

    /// All face uses.
    #[must_use]
    pub fn faceuses(&self) -> &Arena<FaceuseId, Faceuse> {
        &self.faceuses
    }

    /// All loop uses.
    #[must_use]
    pub fn loopuses(&self) -> &Arena<LoopuseId, Loopuse> {
        &self.loopuses
    }

    /// All edge uses.
    #[must_use]
    pub fn edgeuses(&self) -> &Arena<EdgeuseId, Edgeuse> {
        &self.edgeuses
    }

    /// All vertex uses.
    #[must_use]
    pub fn vertexuses(&self) -> &Arena<VertexuseId, Vertexuse> {
        &self.vertexuses
    }

    /// The region `id`.
    pub fn region(&self, id: RegionId) -> Result<&Region> {
        self.regions
            .get(id)
            .ok_or_else(|| not_found("topology::Brep::region", id))
    }

    /// The face use `id`.
    pub fn faceuse(&self, id: FaceuseId) -> Result<&Faceuse> {
        self.faceuses
            .get(id)
            .ok_or_else(|| not_found("topology::Brep::faceuse", id))
    }

    /// The loop use `id`.
    pub fn loopuse(&self, id: LoopuseId) -> Result<&Loopuse> {
        self.loopuses
            .get(id)
            .ok_or_else(|| not_found("topology::Brep::loopuse", id))
    }

    /// The edge use `id`.
    pub fn edgeuse(&self, id: EdgeuseId) -> Result<&Edgeuse> {
        self.edgeuses
            .get(id)
            .ok_or_else(|| not_found("topology::Brep::edgeuse", id))
    }

    /// The vertex use `id`.
    pub fn vertexuse(&self, id: VertexuseId) -> Result<&Vertexuse> {
        self.vertexuses
            .get(id)
            .ok_or_else(|| not_found("topology::Brep::vertexuse", id))
    }

    /// Mutable access to the shell `id`.
    pub fn shell_mut(&mut self, id: ShellId) -> Result<&mut Shell> {
        self.shells
            .get_mut(id)
            .ok_or_else(|| not_found("topology::Brep::shell_mut", id))
    }

    /// Mutable access to the face `id`.
    pub fn face_mut(&mut self, id: FaceId) -> Result<&mut Face> {
        self.faces
            .get_mut(id)
            .ok_or_else(|| not_found("topology::Brep::face_mut", id))
    }

    /// Mutable access to the loop `id`.
    pub fn loop_mut(&mut self, id: LoopId) -> Result<&mut Loop> {
        self.loops
            .get_mut(id)
            .ok_or_else(|| not_found("topology::Brep::loop_mut", id))
    }

    /// Mutable access to the edge `id`.
    pub fn edge_mut(&mut self, id: EdgeId) -> Result<&mut Edge> {
        self.edges
            .get_mut(id)
            .ok_or_else(|| not_found("topology::Brep::edge_mut", id))
    }

    /// Mutable access to the vertex `id`.
    pub fn vertex_mut(&mut self, id: VertexId) -> Result<&mut Vertex> {
        self.vertices
            .get_mut(id)
            .ok_or_else(|| not_found("topology::Brep::vertex_mut", id))
    }

    /// Mutable access to the region `id`.
    pub fn region_mut(&mut self, id: RegionId) -> Result<&mut Region> {
        self.regions
            .get_mut(id)
            .ok_or_else(|| not_found("topology::Brep::region_mut", id))
    }

    /// Mutable access to the face use `id`.
    pub fn faceuse_mut(&mut self, id: FaceuseId) -> Result<&mut Faceuse> {
        self.faceuses
            .get_mut(id)
            .ok_or_else(|| not_found("topology::Brep::faceuse_mut", id))
    }

    /// Mutable access to the loop use `id`.
    pub fn loopuse_mut(&mut self, id: LoopuseId) -> Result<&mut Loopuse> {
        self.loopuses
            .get_mut(id)
            .ok_or_else(|| not_found("topology::Brep::loopuse_mut", id))
    }

    /// Mutable access to the edge use `id`.
    pub fn edgeuse_mut(&mut self, id: EdgeuseId) -> Result<&mut Edgeuse> {
        self.edgeuses
            .get_mut(id)
            .ok_or_else(|| not_found("topology::Brep::edgeuse_mut", id))
    }

    /// Mutable access to the vertex use `id`.
    pub fn vertexuse_mut(&mut self, id: VertexuseId) -> Result<&mut Vertexuse> {
        self.vertexuses
            .get_mut(id)
            .ok_or_else(|| not_found("topology::Brep::vertexuse_mut", id))
    }

    /// `m_bValidateOnly`: while set, tolerances must not change.
    #[must_use]
    pub const fn validate_only(&self) -> bool {
        self.validate_only
    }

    /// Sets `m_bValidateOnly`.
    pub fn set_validate_only(&mut self, validate_only: bool) {
        self.validate_only = validate_only;
    }

    /// `SmBrep::GetTolerance` (`SM_USE_OLDTOL`, CO-PORTED by `topology.edgeuse_p2`):
    /// `m_sThisZoneTol3d`, [`ZONE_TOL_3D`] for a new B-rep (`SmTol::GetZoneTol3d` of
    /// the default context, as `SmBrep()`).
    #[must_use]
    pub const fn tolerance(&self) -> f64 {
        self.tolerance
    }

    /// Sets `m_sThisZoneTol3d` (the raw member write; `SmBrep::SetTolerance`, which
    /// also updates the entities, is unit `topology.brep_*`).
    pub fn set_tolerance(&mut self, tolerance: f64) {
        self.tolerance = tolerance;
    }

    /// The identity the B-rep is attributed and notified under (an `SmBrep*`).
    #[must_use]
    pub const fn key(&self) -> ObjectKey {
        self.key
    }

    /// Sets the identity the B-rep is attributed and notified under.
    pub fn set_key(&mut self, key: ObjectKey) {
        self.key = key;
    }

    /// Appends a region record as is (no validation).
    pub fn add_region_record(&mut self, region: Region) -> Result<RegionId> {
        self.regions.insert(region)
    }

    /// Appends a shell record as is (no validation).
    pub fn add_shell_record(&mut self, shell: Shell) -> Result<ShellId> {
        self.shells.insert(shell)
    }

    /// Appends a face record as is (no validation; the shell's face list is not
    /// touched); the face joins the end of the B-rep's face list.
    pub fn add_face_record(&mut self, face: Face) -> Result<FaceId> {
        let id = self.faces.insert(face)?;
        self.face_list.push(id);
        Ok(id)
    }

    /// Appends a loop record as is (no validation; the face's loop list is not
    /// touched).
    pub fn add_loop_record(&mut self, loop_: Loop) -> Result<LoopId> {
        self.loops.insert(loop_)
    }

    /// Appends an edge record as is (no validation); the edge joins the end of the
    /// B-rep's edge list.
    pub fn add_edge_record(&mut self, edge: Edge) -> Result<EdgeId> {
        let id = self.edges.insert(edge)?;
        self.edge_list.push(id);
        Ok(id)
    }

    /// Appends a vertex record as is; the vertex joins the end of the B-rep's vertex
    /// list.
    pub fn add_vertex_record(&mut self, vertex: Vertex) -> Result<VertexId> {
        let id = self.vertices.insert(vertex)?;
        self.vertex_list.push(id);
        Ok(id)
    }

    /// Appends a face use record as is (no validation).
    pub fn add_faceuse_record(&mut self, faceuse: Faceuse) -> Result<FaceuseId> {
        self.faceuses.insert(faceuse)
    }

    /// Appends a loop use record as is (no validation).
    pub fn add_loopuse_record(&mut self, loopuse: Loopuse) -> Result<LoopuseId> {
        self.loopuses.insert(loopuse)
    }

    /// Appends an edge use record as is (no validation).
    pub fn add_edgeuse_record(&mut self, edgeuse: Edgeuse) -> Result<EdgeuseId> {
        self.edgeuses.insert(edgeuse)
    }

    /// Appends a vertex use record as is (no validation).
    pub fn add_vertexuse_record(&mut self, vertexuse: Vertexuse) -> Result<VertexuseId> {
        self.vertexuses.insert(vertexuse)
    }

    /// Adds an empty shell.
    pub fn add_shell(&mut self) -> Result<ShellId> {
        self.shells.insert(Shell::default())
    }

    /// Adds a vertex at `point`.
    pub fn add_vertex(&mut self, point: Point3) -> Result<VertexId> {
        self.add_vertex_record(Vertex {
            point,
            tolerance: ZONE_TOL_3D,
            ..Vertex::default()
        })
    }

    /// Adds an edge from `start` to `end`; both vertices must exist.
    pub fn add_edge(&mut self, start: VertexId, end: VertexId) -> Result<EdgeId> {
        const OP: &str = "topology::Brep::add_edge";
        for v in [start, end] {
            if self.vertices.get(v).is_none() {
                return Err(not_found(OP, v));
            }
        }
        self.add_edge_record(Edge {
            vertices: [start, end],
            edgeuses: Vec::new(),
            interval: Extent1d::new(),
            tolerance: ZONE_TOL_3D,
            curve: None,
        })
    }

    /// Adds a face to `shell` with no loops yet.
    pub fn add_face(&mut self, shell: ShellId) -> Result<FaceId> {
        const OP: &str = "topology::Brep::add_face";
        if self.shells.get(shell).is_none() {
            return Err(not_found(OP, shell));
        }
        let face = self.add_face_record(Face::new(shell))?;
        self.shells
            .get_mut(shell)
            .ok_or_else(|| not_found(OP, shell))?
            .faces
            .push(face);
        Ok(face)
    }

    /// Adds a loop to `face` from edges in traversal order. Every edge must exist and
    /// consecutive uses must share a vertex, closing back to the first.
    pub fn add_loop(&mut self, face: FaceId, edges: Vec<LoopEdge>) -> Result<LoopId> {
        const OP: &str = "topology::Brep::add_loop";
        if self.faces.get(face).is_none() {
            return Err(not_found(OP, face));
        }
        let mut ends = Vec::with_capacity(edges.len());
        for use_ in &edges {
            let e = self
                .edges
                .get(use_.edge)
                .ok_or_else(|| not_found(OP, use_.edge))?;
            let [a, b] = e.vertices;
            ends.push(if use_.forward { (a, b) } else { (b, a) });
        }
        let closes = ends
            .iter()
            .zip(ends.iter().cycle().skip(1))
            .all(|(cur, next)| cur.1 == next.0);
        if ends.is_empty() || !closes {
            return Err(SolidError::InvalidInput {
                operation: OP,
                reason: "loop edges do not close",
            });
        }
        let id = self.loops.insert(Loop {
            face,
            edges,
            loopuse: None,
        })?;
        self.faces
            .get_mut(face)
            .ok_or_else(|| not_found(OP, face))?
            .loops
            .push(id);
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Vec3;

    #[test]
    fn build_single_triangle_face() {
        let mut b = Brep::new();
        let s = b.add_shell().unwrap();
        let v: Vec<VertexId> = [Vec3::ZERO, Vec3::X, Vec3::Y]
            .into_iter()
            .map(|p| b.add_vertex(p).unwrap())
            .collect();
        let e0 = b.add_edge(v[0], v[1]).unwrap();
        let e1 = b.add_edge(v[1], v[2]).unwrap();
        let e2 = b.add_edge(v[0], v[2]).unwrap();
        let f = b.add_face(s).unwrap();
        let uses = vec![
            LoopEdge {
                edge: e0,
                forward: true,
            },
            LoopEdge {
                edge: e1,
                forward: true,
            },
            LoopEdge {
                edge: e2,
                forward: false,
            },
        ];
        let l = b.add_loop(f, uses).unwrap();

        assert_eq!(b.vertices().len(), 3);
        assert_eq!(b.edges().ids().collect::<Vec<_>>(), vec![e0, e1, e2]);
        assert_eq!(b.face(f).unwrap().loops, vec![l]);
        assert_eq!(b.shell(s).unwrap().faces, vec![f]);
        assert_eq!(b.loop_(l).unwrap().face, f);
        assert_eq!(b.vertex(v[1]).unwrap().point, Vec3::X);
        assert_eq!(b.edge(e2).unwrap().vertices, [v[0], v[2]]);
        let (first_id, first) = b.vertices().iter().next().unwrap();
        assert_eq!((first_id, first.point), (v[0], Vec3::ZERO));
    }

    #[test]
    fn errors_name_operation_and_entity() {
        let mut b = Brep::new();
        let missing = VertexId::from_index(3);
        let err = b.add_edge(missing, missing).unwrap_err();
        assert_eq!(err.operation(), "topology::Brep::add_edge");
        assert_eq!(err.entity(), Some(EntityRef::Vertex(missing)));
        assert!(b.face(FaceId::from_index(0)).is_err());

        let s = b.add_shell().unwrap();
        let f = b.add_face(s).unwrap();
        let v0 = b.add_vertex(Vec3::ZERO).unwrap();
        let v1 = b.add_vertex(Vec3::X).unwrap();
        let e = b.add_edge(v0, v1).unwrap();
        let open = vec![LoopEdge {
            edge: e,
            forward: true,
        }];
        assert!(matches!(
            b.add_loop(f, open),
            Err(SolidError::InvalidInput { .. })
        ));
        assert!(b.add_loop(f, Vec::new()).is_err());
        assert!(b.loops().is_empty());
    }
}
