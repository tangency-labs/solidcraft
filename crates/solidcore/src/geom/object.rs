//! The kernel's object base: run-time type codes, change notification through the
//! context callbacks, the scoped "temporarily change a value" guard and shapes
//! (sets of faces, edges and vertices with spatial trees of their geometry).
//!
//! Upstream: `source/SMLib/inc/SmObject.h`, `source/SMLib/src/SmObject.cpp`,
//! `source/SMLib/inc/SmShape.h`, `source/SMLib/src/SmShape.cpp`,
//! `source/SMLib/src/SmThreadLocalStorage.cpp`.
//!
//! | Rust                                         | Upstream                                          |
//! |----------------------------------------------|---------------------------------------------------|
//! | [`OBJECT_TYPE`], [`OBJECT_TYPE_STRING`]      | `SmObject::GetType`, `SmObject::GetTypeString`    |
//! | [`is_kind_of`]                               | `SmObject::IsKindOf` (`SM_COMMON_BASE`)           |
//! | [`ObjectRef`]                                | an `SmObject*` argument of `Notify` / `Execute`   |
//! | [`NotifyOperation`]                          | `SmNotifyOperation`                               |
//! | [`notify`]                                   | `SmObject::Notify`                                |
//! | [`assert_valid`]                             | `SmObject::AssertValid`                           |
//! | [`TemporaryChangeValue`]                     | `SmTemporaryChangeValue<TYPE>`                    |
//! | [`TemporaryChangeValue::empty`]              | `SmTemporaryChangeValue()`                        |
//! | [`TemporaryChangeValue::new`]                | `SmTemporaryChangeValue(TYPE&, TYPE)`             |
//! | [`TemporaryChangeValue::set`]                | `SmTemporaryChangeValue::Set`                     |
//! | [`TemporaryChangeValue::clear`]              | `SmTemporaryChangeValue::Clear`                   |
//! | [`TemporaryChangeValue::use_orig_value`]     | `SmTemporaryChangeValue::UseOrigValue`            |
//! | [`TemporaryChangeValue::use_temp_value`]     | `SmTemporaryChangeValue::UseTempValue`            |
//! | [`TemporaryChangeValue::set_new_value`]      | `SmTemporaryChangeValue::SetNewValue`             |
//! | `Drop for TemporaryChangeValue`              | `~SmTemporaryChangeValue`                         |
//! | [`Shape`], [`Shape::new`]                    | `SmShape`, `SmShape::SmShape`                     |
//! | `Drop` (implicit)                            | `SmShape::~SmShape`                               |
//! | [`Shape::vertex_tree`], [`Shape::curve_tree`], [`Shape::surface_tree`] | `SmShape::GetVertexTree`, `GetCurveTree`, `GetSurfaceTree` |
//! | [`Shape::faces`], [`Shape::edges`], [`Shape::vertices`] | `SmShape::GetFaces`, `GetEdges`, `GetVertices` |
//! | [`Shape::is_in_shape`]                       | `SmShape::IsInShape`                              |
//! | [`Shape::TYPE`], [`Shape::type_code`], [`Shape::type_string`], [`Shape::is_kind_of`] | `SmShape::GetType`, `GetTypeString`, `IsKindOf` |
//!
//! Deviations:
//! - **Object identity and context.** There is no common object base class: an
//!   object taking part in a notification is an [`ObjectRef`] (its type code plus
//!   an opaque [`ObjectKey`] chosen by its owner), and the context an object would
//!   reach through `m_cpContext` is passed explicitly (`None` for an object with a
//!   null context). `SmObject::GetContext` / `SetContext`, the constructors and
//!   destructor, the overloaded `operator new` / `delete` and the thread-local
//!   "been through new" handshake that fed them (`SmThreadLocalStorage::
//!   SetBeenThroughNew` / `GetBeenThroughNew`) therefore have no counterpart.
//! - `SmThreadLocalStorage::GetErrCallbackFunction` (a thread-local error callback
//!   pointer) has no counterpart: errors are returned as [`crate::SolidError`]
//!   values and the port keeps no global or thread-local state.
//! - `SmObjDelete` and `SmTypedArrayDelete<TYPE>` (scoped `delete` helpers) are
//!   Rust ownership (`Box`, `Vec`, `Option::take`); nothing to port.
//! - `SmTemporaryChangeValue` holds the guarded value as a `&mut T` borrow; while
//!   the guard lives the value is read and written through
//!   [`TemporaryChangeValue::get`] / [`TemporaryChangeValue::get_mut`], and a
//!   nested guard of the same value borrows it from the outer guard's `get_mut`.
//!   `UseOrigValue` / `UseTempValue` on a guard that was never `Set` dereference
//!   a null pointer upstream; here they do nothing.
//! - `Notify`'s debug echo (`SM_ECHO_NOTIFY`), `SmObject::Dump`, `SmShape::Draw`
//!   and `GetObjectColor` print or draw only and are not ported. Callback status
//!   values are ignored, as upstream.
//! - The shape's topology members are `(BrepId, id)` pairs over a caller-owned
//!   B-rep slice. Building a non-empty shape needs the topology and geometry
//!   queries of units not ported yet and returns [`SolidError::Unported`]; the
//!   empty shape (upstream's `SE_MSG` "no Topology Objects" case, whose message is
//!   not ported) is built exactly as upstream.

use crate::error::{Result, SolidError};
use crate::geom::attribute::ObjectKey;
use crate::geom::context::Context;
use crate::geom::extent3d::Extent3d;
use crate::geom::tree::Tree;
use crate::math::Vec3;
use crate::topology::{BrepId, EdgeId, FaceId, VertexId};

/// `SmObject_TYPE` (`OS_BASE_TYPE + 1`).
pub const OBJECT_TYPE: i64 = 11001;

/// `SmObject::GetTypeString`.
pub const OBJECT_TYPE_STRING: &str = "SmObject_TYPE";

/// `SmObject::IsKindOf`: true only for [`OBJECT_TYPE`]. Derived classes check their
/// own type first and end their chain here.
#[must_use]
pub const fn is_kind_of(t: i64) -> bool {
    t == OBJECT_TYPE
}

/// `SmObject::AssertValid`: the base object has nothing to check.
#[must_use]
pub const fn assert_valid() -> bool {
    true
}

/// An object passed to a notification: its upstream type code (`GetType`) and the
/// key its owner identifies it by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectRef {
    /// The object's `SM_TYPE` code.
    pub type_code: i64,
    /// The object's identity.
    pub key: ObjectKey,
}

/// The kind of change a notification reports (`SmNotifyOperation`). The
/// discriminants are the upstream values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum NotifyOperation {
    /// `SM_NO_ADD_TO_BREP`: a topology object was added to a B-rep (caller the B-rep;
    /// data the added object, the B-rep, the added object's geometry).
    AddToBrep = 0,
    /// `SM_NO_SPLIT_IN_BREP`: a topology object was split in a B-rep (data the
    /// original object and the two children).
    SplitInBrep = 1,
    /// `SM_NO_MERGE_IN_BREP`: two topology objects were merged in a B-rep (data the
    /// surviving object, the deleted object, the B-rep).
    MergeInBrep = 2,
    /// `SM_NO_TRIM_NO_SPLIT_IN_BREP`: a boundary object was added to a target object
    /// without splitting it (data the target and the boundary object).
    TrimNoSplitInBrep = 3,
    /// `SM_NO_COINCIDENT`: objects of two B-reps were found coincident (caller B-rep
    /// A; data its object, B-rep B's object, B-rep B).
    Coincident = 4,
    /// `SM_NO_RM_FROM_BREP`: a topology object was removed from a B-rep (data the
    /// removed object, the B-rep, the removed object's geometry).
    RmFromBrep = 5,
    /// `SM_NO_CHANGE_GEOMETRY`: a topology object's geometry changed (data the new
    /// geometry, the B-rep, the old geometry).
    ChangeGeometry = 6,
    /// `SM_NO_CHANGE_OWNER`: a geometry object's owner changed (data the new owner,
    /// its B-rep, the old owner).
    ChangeOwner = 7,
    /// `SM_NO_CONSTRUCTION`: an object was constructed (caller and first data the
    /// new object; second data the object it was copied from).
    Construction = 8,
    /// `SM_NO_COPY`: an object was copied (caller the source; data the copy, the
    /// copy's owner, the source's owner).
    Copy = 9,
    /// `SM_NO_PRE_EDIT`: an object is about to be edited (caller and first data the
    /// object; second data its owner).
    PreEdit = 10,
    /// `SM_NO_POST_EDIT`: an object was edited (caller and first data the object;
    /// second data its owner).
    PostEdit = 11,
    /// `SM_NO_SPLIT`: a geometry object was split (data the two children and the
    /// split object's owner).
    Split = 12,
    /// `SM_NO_MERGE`: two geometry objects were merged (data the two originals and
    /// the merged object's owner).
    Merge = 13,
    /// `SM_NO_REG_PROPAGATION`: a Boolean combined regions of two B-reps (caller
    /// the merge region; data this B-rep's regions, the other B-rep's source
    /// regions, this B-rep's merged region).
    RegPropagation = 14,
    /// `SM_NO_DESTRUCTION`: an object is being deleted (caller and first data the
    /// object).
    Destruction = 15,
    /// `SM_NO_UNKNOWN`: uninitialised value.
    Unknown = 16,
}

impl NotifyOperation {
    /// Every operation, in upstream order.
    pub const ALL: [Self; 17] = [
        Self::AddToBrep,
        Self::SplitInBrep,
        Self::MergeInBrep,
        Self::TrimNoSplitInBrep,
        Self::Coincident,
        Self::RmFromBrep,
        Self::ChangeGeometry,
        Self::ChangeOwner,
        Self::Construction,
        Self::Copy,
        Self::PreEdit,
        Self::PostEdit,
        Self::Split,
        Self::Merge,
        Self::RegPropagation,
        Self::Destruction,
        Self::Unknown,
    ];

    /// The upstream enum value.
    #[must_use]
    pub const fn code(self) -> i32 {
        self as i32
    }

    /// The operation with upstream value `code`, if any.
    #[must_use]
    pub fn from_code(code: i32) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.code() == code)
    }
}

/// `SmObject::Notify`: reports `operation` on `caller` to the context's user
/// notify callback, then to its system notify callback (each only if set). An
/// object with a null context (`context` `None`) notifies nobody. The callbacks'
/// status values are ignored, as upstream.
pub fn notify(
    context: Option<&mut Context>,
    operation: NotifyOperation,
    caller: ObjectRef,
    data1: Option<ObjectRef>,
    data2: Option<ObjectRef>,
    data3: Option<ObjectRef>,
) {
    let Some(context) = context else {
        return;
    };
    // Call user callback if present.
    if let Some(ucb) = context.user_notify_callback_mut() {
        let _status = ucb.execute(operation, caller, data1, data2, data3);
    }
    // Call system callback if present.
    if let Some(scb) = context.sys_notify_callback_mut() {
        let _status = scb.execute(operation, caller, data1, data2, data3);
    }
}

/// A scoped change of a value (`SmTemporaryChangeValue<TYPE>`): sets the value on
/// [`TemporaryChangeValue::set`] and puts the old value back when dropped, unless
/// [`TemporaryChangeValue::clear`]ed.
#[derive(Debug)]
pub struct TemporaryChangeValue<'a, T: Copy + PartialEq> {
    /// `m_pOriginalPtr`.
    target: Option<&'a mut T>,
    /// `m_vOldValue`: restored on drop.
    old_value: Option<T>,
    /// `m_vNewValue`: the value used while the guard lives.
    new_value: Option<T>,
    /// `m_bEnabled`.
    enabled: bool,
}

impl<T: Copy + PartialEq> Default for TemporaryChangeValue<'_, T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<'a, T: Copy + PartialEq> TemporaryChangeValue<'a, T> {
    /// A guard holding no value yet (`SmTemporaryChangeValue()`); call
    /// [`TemporaryChangeValue::set`] to use it.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            target: None,
            old_value: None,
            new_value: None,
            enabled: false,
        }
    }

    /// Changes `*target` to `new_value` until the guard is dropped
    /// (`SmTemporaryChangeValue(TYPE&, TYPE)`).
    pub fn new(target: &'a mut T, new_value: T) -> Self {
        let mut guard = Self::empty();
        guard.set(target, new_value);
        guard
    }

    /// `Set`: remembers `*target` as the value to restore, enables the guard and
    /// writes `new_value` when it differs. A previously held value is released
    /// without being restored, as upstream.
    pub fn set(&mut self, target: &'a mut T, new_value: T) {
        self.old_value = Some(*target);
        self.new_value = Some(new_value);
        self.enabled = true;
        if *target != new_value {
            *target = new_value;
        }
        self.target = Some(target);
    }

    /// `Clear`: the value is no longer restored on drop.
    pub fn clear(&mut self) {
        self.enabled = false;
    }

    /// `UseOrigValue`: writes the remembered old value (no-op without a value).
    pub fn use_orig_value(&mut self) {
        if let (Some(target), Some(old)) = (self.target.as_deref_mut(), self.old_value) {
            *target = old;
        }
    }

    /// `UseTempValue`: writes the temporary value (no-op without a value).
    pub fn use_temp_value(&mut self) {
        if let (Some(target), Some(new)) = (self.target.as_deref_mut(), self.new_value) {
            *target = new;
        }
    }

    /// `SetNewValue`: changes the temporary value used by
    /// [`TemporaryChangeValue::use_temp_value`] (does not write it).
    pub fn set_new_value(&mut self, new_value: T) {
        self.new_value = Some(new_value);
    }

    /// Whether the old value will be restored on drop (`m_bEnabled`).
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The guarded value, if the guard holds one.
    #[must_use]
    pub fn get(&self) -> Option<&T> {
        self.target.as_deref()
    }

    /// The guarded value for writing, if the guard holds one.
    pub fn get_mut(&mut self) -> Option<&mut T> {
        self.target.as_deref_mut()
    }
}

impl<T: Copy + PartialEq> Drop for TemporaryChangeValue<'_, T> {
    /// `~SmTemporaryChangeValue`: restores the old value when enabled and changed.
    fn drop(&mut self) {
        if !self.enabled {
            return;
        }
        if let (Some(target), Some(old)) = (self.target.as_deref_mut(), self.old_value) {
            if *target != old {
                *target = old;
            }
        }
    }
}

/// A face of a shape: the B-rep it belongs to and its id there.
pub type FaceKey = (BrepId, FaceId);
/// An edge of a shape: the B-rep it belongs to and its id there.
pub type EdgeKey = (BrepId, EdgeId);
/// A vertex of a shape: the B-rep it belongs to and its id there.
pub type VertexKey = (BrepId, VertexId);

/// A topology object asked about by [`Shape::is_in_shape`] (an upstream
/// `SmTopology*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShapeMember {
    /// An `SmFace`.
    Face(FaceKey),
    /// An `SmEdge`.
    Edge(EdgeKey),
    /// An `SmVertex`.
    Vertex(VertexKey),
    /// Any other topology object (never in a shape).
    Other,
}

/// A collection of faces, edges and vertices, possibly from several B-reps, with
/// spatial trees of their vertices, edge curves and face surfaces (`SmShape`).
/// Adding a B-rep adds all its faces, edges and vertices; adding a face adds its
/// edges and vertices; adding an edge adds its vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// `m_pVertexTree`: spatial tree of every vertex.
    vertex_tree: Tree<VertexKey>,
    /// `m_pCurveTree`: spatial tree of every edge curve (keyed by the edge).
    curve_tree: Tree<EdgeKey>,
    /// `m_pSurfaceTree`: spatial tree of every face surface (keyed by the face).
    surface_tree: Tree<FaceKey>,
    /// `m_vFaces`.
    faces: Vec<FaceKey>,
    /// `m_vEdges`: the listed edges plus the edges of every face.
    edges: Vec<EdgeKey>,
    /// `m_vVertices`: the listed vertices plus those of every face and edge.
    vertices: Vec<VertexKey>,
}

impl Shape {
    /// `SmShape_TYPE` (`TOPO_BASE_TYPE + 500`).
    pub const TYPE: i64 = 16500;

    /// Builds a shape from whole B-reps, faces, edges and vertices
    /// (`SmShape::SmShape`). The lower-level members of a listed entity need not be
    /// listed again.
    ///
    /// With every list empty the shape is empty and its three trees cover the box
    /// `(0,0,0)-(1,1,1)` (upstream also reports an `SE_MSG` error and continues).
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] for a non-empty shape: its bounding boxes, marks and
    /// tree loading need the B-rep, face, edge, vertex and geometry queries of
    /// units not ported yet.
    pub fn new(
        breps: &[BrepId],
        faces: &[FaceKey],
        edges: &[EdgeKey],
        vertices: &[VertexKey],
    ) -> Result<Self> {
        // Compute the union of all of the extents of the input objects; the first
        // upstream call of each list needs an unported topology query.
        if !breps.is_empty() {
            // UNPORTED(topology.brep_p1): SmBrep::GetInfiniteRegion (then
            // SmBrep::CalculateBoundingBox, SmBrep::GetVertices / GetEdges / GetFaces)
            return Err(SolidError::Unported {
                upstream: "SmBrep::GetInfiniteRegion",
                unit: "topology.brep_p1",
            });
        }
        if !faces.is_empty() {
            // UNPORTED(query.face_p1): SmFace::CalculateBoundingBox (then
            // SmFace::GetEdges / GetVertices). GetSurface/GetFace are ported.
            return Err(SolidError::Unported {
                upstream: "SmFace::CalculateBoundingBox",
                unit: "query.face_p1",
            });
        }
        if !edges.is_empty() {
            // UNPORTED(topology.edge): SmEdge::GetCurve (then SmCurve::GetOwner,
            // SmEdge::CalculateBoundingBox, SmEdge::GetVertices)
            return Err(SolidError::Unported {
                upstream: "SmEdge::GetCurve",
                unit: "topology.edge",
            });
        }
        if !vertices.is_empty() {
            // UNPORTED(topology.vertex): SmVertex::GetPoint (then
            // SmTopology::IsMarked / Mark)
            return Err(SolidError::Unported {
                upstream: "SmVertex::GetPoint",
                unit: "topology.vertex",
            });
        }

        // signal an error for empty shapes: specify BBox to be anything reasonable
        let mut bbox_union = Extent3d::new();
        bbox_union.set_min_max(Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0))?;

        // init trees for each topology item to be stored; nothing to load
        Ok(Self {
            vertex_tree: Tree::new_spatial(&bbox_union),
            curve_tree: Tree::new_spatial(&bbox_union),
            surface_tree: Tree::new_spatial(&bbox_union),
            faces: Vec::new(),
            edges: Vec::new(),
            vertices: Vec::new(),
        })
    }

    /// The spatial tree of the shape's vertices (`GetVertexTree`).
    #[must_use]
    pub const fn vertex_tree(&self) -> &Tree<VertexKey> {
        &self.vertex_tree
    }

    /// The spatial tree of the shape's edge curves (`GetCurveTree`).
    #[must_use]
    pub const fn curve_tree(&self) -> &Tree<EdgeKey> {
        &self.curve_tree
    }

    /// The spatial tree of the shape's face surfaces (`GetSurfaceTree`).
    #[must_use]
    pub const fn surface_tree(&self) -> &Tree<FaceKey> {
        &self.surface_tree
    }

    /// The shape's faces (`GetFaces`).
    #[must_use]
    pub fn faces(&self) -> &[FaceKey] {
        &self.faces
    }

    /// The shape's edges (`GetEdges`).
    #[must_use]
    pub fn edges(&self) -> &[EdgeKey] {
        &self.edges
    }

    /// The shape's vertices (`GetVertices`).
    #[must_use]
    pub fn vertices(&self) -> &[VertexKey] {
        &self.vertices
    }

    /// Whether a face, edge or vertex is in the shape (`IsInShape`). B-reps were
    /// broken down into their faces, edges and vertices, so any other topology
    /// object is never in a shape.
    #[must_use]
    pub fn is_in_shape(&self, member: ShapeMember) -> bool {
        match member {
            ShapeMember::Face(f) => self.faces.contains(&f),
            ShapeMember::Edge(e) => self.edges.contains(&e),
            ShapeMember::Vertex(v) => self.vertices.contains(&v),
            ShapeMember::Other => false,
        }
    }

    /// `SmShape_TYPE` (`GetType`).
    #[must_use]
    pub const fn type_code(&self) -> i64 {
        Self::TYPE
    }

    /// `"SmShape_TYPE"` (`GetTypeString`).
    #[must_use]
    pub const fn type_string(&self) -> &'static str {
        "SmShape_TYPE"
    }

    /// True for `SmShape_TYPE` and its base `SmObject_TYPE` (`IsKindOf`).
    #[must_use]
    pub const fn is_kind_of(&self, t: i64) -> bool {
        t == Self::TYPE || is_kind_of(t)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::geom::context::NotifyCallback;
    use crate::geom::tree::NodeId;

    type Log = Arc<Mutex<Vec<(u8, NotifyOperation, ObjectRef, [Option<ObjectRef>; 3])>>>;

    #[derive(Debug)]
    struct Recorder {
        tag: u8,
        log: Log,
    }

    impl NotifyCallback for Recorder {
        fn execute(
            &mut self,
            operation: NotifyOperation,
            caller: ObjectRef,
            data1: Option<ObjectRef>,
            data2: Option<ObjectRef>,
            data3: Option<ObjectRef>,
        ) -> Result<()> {
            self.log
                .lock()
                .unwrap()
                .push((self.tag, operation, caller, [data1, data2, data3]));
            Err(SolidError::InvalidInput {
                operation: "test",
                reason: "ignored",
            })
        }
    }

    fn obj(k: u64) -> ObjectRef {
        ObjectRef {
            type_code: OBJECT_TYPE,
            key: ObjectKey(k),
        }
    }

    #[test]
    fn object_type_system() {
        assert!(is_kind_of(OBJECT_TYPE));
        assert!(!is_kind_of(Shape::TYPE));
        assert!(assert_valid());
        assert_eq!(OBJECT_TYPE_STRING, "SmObject_TYPE");
        for op in NotifyOperation::ALL {
            assert_eq!(NotifyOperation::from_code(op.code()), Some(op));
        }
        assert_eq!(NotifyOperation::from_code(17), None);
        assert_eq!(NotifyOperation::Destruction.code(), 15);
    }

    #[test]
    fn notify_calls_user_then_system() {
        let log: Log = Arc::default();
        let mut ctx = Context::default();
        notify(
            Some(&mut ctx),
            NotifyOperation::Copy,
            obj(1),
            Some(obj(2)),
            None,
            None,
        );
        assert!(log.lock().unwrap().is_empty());
        ctx.set_sys_notify_callback(Some(Box::new(Recorder {
            tag: 2,
            log: log.clone(),
        })));
        ctx.set_user_notify_callback(Some(Box::new(Recorder {
            tag: 1,
            log: log.clone(),
        })));
        notify(
            Some(&mut ctx),
            NotifyOperation::Split,
            obj(1),
            Some(obj(2)),
            Some(obj(3)),
            None,
        );
        notify(None, NotifyOperation::Merge, obj(9), None, None, None);
        let l = log.lock().unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].0, 1);
        assert_eq!(l[1].0, 2);
        assert_eq!(l[1].1, NotifyOperation::Split);
        assert_eq!(l[1].2, obj(1));
        assert_eq!(l[1].3, [Some(obj(2)), Some(obj(3)), None]);
    }

    #[test]
    fn temporary_change_value_restores() {
        let mut b = 5.0;
        {
            let mut g = TemporaryChangeValue::new(&mut b, 25.0);
            assert_eq!(g.get(), Some(&25.0));
            {
                let inner = TemporaryChangeValue::new(g.get_mut().unwrap(), 7.0);
                assert_eq!(inner.get(), Some(&7.0));
            }
            assert_eq!(g.get(), Some(&25.0));
            g.use_orig_value();
            assert_eq!(g.get(), Some(&5.0));
            g.set_new_value(3.0);
            g.use_temp_value();
            assert_eq!(g.get(), Some(&3.0));
        }
        assert_eq!(b, 5.0);
        {
            let mut g = TemporaryChangeValue::new(&mut b, 1.0);
            g.clear();
            assert!(!g.is_enabled());
        }
        assert_eq!(b, 1.0);
        {
            let mut g: TemporaryChangeValue<'_, f64> = TemporaryChangeValue::empty();
            g.use_orig_value();
            g.use_temp_value();
            assert_eq!(g.get(), None);
            g.set(&mut b, 2.0);
        }
        assert_eq!(b, 1.0);
    }

    #[test]
    fn empty_shape() {
        let s = Shape::new(&[], &[], &[], &[]).unwrap();
        let top = s.vertex_tree().node(NodeId(0)).unwrap();
        assert_eq!(top.bbox.min(), Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(top.bbox.max(), Vec3::new(1.0, 1.0, 1.0));
        assert_eq!(s.curve_tree().nodes().len(), 1);
        assert_eq!(s.surface_tree().nodes().len(), 1);
        assert!(s.faces().is_empty() && s.edges().is_empty() && s.vertices().is_empty());
        let b = BrepId::from_index(0);
        assert!(!s.is_in_shape(ShapeMember::Face((b, FaceId::from_index(0)))));
        assert!(!s.is_in_shape(ShapeMember::Other));
        assert!(s.is_kind_of(Shape::TYPE) && s.is_kind_of(OBJECT_TYPE) && !s.is_kind_of(1));
        assert_eq!((s.type_code(), s.type_string()), (16500, "SmShape_TYPE"));
    }

    #[test]
    fn non_empty_shape_is_unported() {
        let b = BrepId::from_index(0);
        let unit = |r: Result<Shape>| match r {
            Err(SolidError::Unported { unit, .. }) => unit,
            _ => "",
        };
        assert_eq!(
            unit(Shape::new(&[b], &[(b, FaceId::from_index(0))], &[], &[])),
            "topology.brep_p1"
        );
        assert_eq!(
            unit(Shape::new(&[], &[(b, FaceId::from_index(0))], &[], &[])),
            "query.face_p1"
        );
        assert_eq!(
            unit(Shape::new(&[], &[], &[(b, EdgeId::from_index(0))], &[])),
            "topology.edge"
        );
        assert_eq!(
            unit(Shape::new(&[], &[], &[], &[(b, VertexId::from_index(0))])),
            "topology.vertex"
        );
    }
}
