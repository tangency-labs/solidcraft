//! Arena-based B-rep topology with typed ids.
//!
//! Upstream: `source/SMLib/inc/SmBrep.h`, `SmRegion.h`, `SmShell.h`, `SmFace.h`,
//! `SmLoop.h`, `SmEdge.h`, `SmVertex.h`, the use records `SmFaceuse.h`, `SmLoopuse.h`,
//! `SmEdgeuse.h`, `SmVertexuse.h`, and `SmTopology.h` / `SmOwningTopology.h`.
//!
//! | Rust                    | Upstream                                              |
//! |-------------------------|-------------------------------------------------------|
//! | [`Brep`]                | `SmBrep` (face/edge/vertex lists `m_p*ListHead`)      |
//! | [`Shell`] / [`ShellId`] | `SmShell`                                             |
//! | [`Face`] / [`FaceId`]   | `SmFace`                                              |
//! | [`Loop`] / [`LoopId`]   | `SmLoop`                                              |
//! | [`Edge`] / [`EdgeId`]   | `SmEdge`                                              |
//! | [`Vertex`] / [`VertexId`] | `SmVertex`                                          |
//! | [`Arena`]               | upstream's intrusive `SmListNode` linked lists        |
//!
//! Upstream links entities with raw pointers in intrusive lists; the port stores each
//! kind in a flat [`Arena`] owned by the [`Brep`] and links them with typed ids, so
//! iteration order is insertion order (deterministic) and there is no shared
//! ownership. This file is the **skeleton** shared by every unit: the record fields
//! below are the minimum the scaffolding needs. The topology units in
//! `plan/modules.toml` (regions, face/loop/edge/vertex uses, geometry binding,
//! tolerances, attributes) extend these records; keep the id/arena/accessor shape.

pub mod database_io;
#[allow(clippy::module_inception)]
pub mod topology;

use core::marker::PhantomData;

use crate::error::{EntityRef, Result, SolidError};
use crate::math::Point3;

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

/// A vertex (`SmVertex`): a point where edges meet.
#[derive(Debug, Clone, PartialEq)]
pub struct Vertex {
    /// Position in model space.
    pub point: Point3,
}

/// An edge (`SmEdge`): a bounded curve between two vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    /// Start and end vertex (equal for a closed edge).
    pub vertices: [VertexId; 2],
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
}

/// A face (`SmFace`): a bounded region of a surface.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// Shell containing this face.
    pub shell: ShellId,
    /// Boundary loops; upstream keeps the outer loop first.
    pub loops: Vec<LoopId>,
}

/// A shell (`SmShell`): a connected set of faces.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shell {
    /// Faces of the shell, in insertion order.
    pub faces: Vec<FaceId>,
}

/// A boundary representation (`SmBrep`): arenas of shells, faces, loops, edges and
/// vertices linked by typed ids.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Brep {
    shells: Arena<ShellId, Shell>,
    faces: Arena<FaceId, Face>,
    loops: Arena<LoopId, Loop>,
    edges: Arena<EdgeId, Edge>,
    vertices: Arena<VertexId, Vertex>,
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

    /// Adds an empty shell.
    pub fn add_shell(&mut self) -> Result<ShellId> {
        self.shells.insert(Shell::default())
    }

    /// Adds a vertex at `point`.
    pub fn add_vertex(&mut self, point: Point3) -> Result<VertexId> {
        self.vertices.insert(Vertex { point })
    }

    /// Adds an edge from `start` to `end`; both vertices must exist.
    pub fn add_edge(&mut self, start: VertexId, end: VertexId) -> Result<EdgeId> {
        const OP: &str = "topology::Brep::add_edge";
        for v in [start, end] {
            if self.vertices.get(v).is_none() {
                return Err(not_found(OP, v));
            }
        }
        self.edges.insert(Edge {
            vertices: [start, end],
        })
    }

    /// Adds a face to `shell` with no loops yet.
    pub fn add_face(&mut self, shell: ShellId) -> Result<FaceId> {
        const OP: &str = "topology::Brep::add_face";
        if self.shells.get(shell).is_none() {
            return Err(not_found(OP, shell));
        }
        let face = self.faces.insert(Face {
            shell,
            loops: Vec::new(),
        })?;
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
        let id = self.loops.insert(Loop { face, edges })?;
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
