//! Exact face and edge history: for each face and edge of an operation's result, what it came
//! from among the operation's operands, recorded by the code that built it (never sampled).
//!
//! Indices are canonical face and edge indices (docs/native-brep.md, "Canonical order"), the
//! positions in a body's face order and edge order. Operand numbers are positions in the
//! operation's own operand list, which each operation documents. An operation returns its
//! history beside the B-rep; a backend that cannot report it returns none and never guesses.
//! These are the runtime types; the saved form is [`super::envelope`], kept separate so that a
//! change here cannot redefine saved fields. `solidcraft_kernel::history` re-exports them and
//! adds what needs a kernel body.

use std::collections::{BTreeSet, HashMap};

/// Why a history could not be composed or checked.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("history: {0}")]
pub struct Failed(pub &'static str);

type Result<T> = std::result::Result<T, Failed>;

fn failed(detail: &'static str) -> Failed {
    Failed(detail)
}

/// Where a result face came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum FaceOrigin {
    /// A face of input `operand` (its index in the operand's face order).
    Face { operand: usize, face: usize },
    /// Swept from segment `segment` of the profile's loops (in order: outer, then holes).
    Profile { segment: usize },
    /// The start or end cap of a sweep (the profile itself, or where it ends up).
    Cap { end: bool },
    /// Made in place of edge `edge` of input `operand` (a fillet or chamfer).
    Edge { operand: usize, edge: usize },
    /// An offset of face `face` of input `operand` (a shell's inner wall).
    Offset { operand: usize, face: usize },
}

/// How a result face relates to its origins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Change {
    /// The same face as its origin (same region).
    Kept,
    /// Its origin's surface, trimmed differently.
    Modified,
    /// One of several result faces made from one input face.
    Split,
    /// Made from several input faces (coplanar or cosurface faces joined).
    Merged,
    /// New to the operation (no input face under it).
    Generated,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FaceProv {
    pub origins: Vec<FaceOrigin>,
    pub change: Change,
}

/// One side of a meeting: the origins of the face on that side of the edge. One origin, or
/// several where that face was made from several (a merged face) and the edge lies along it
/// as a whole. Never empty; sorted, without repeats, so equal sides compare equal.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct Side(Vec<FaceOrigin>);

impl Side {
    /// The side made of `origins` (in any order, repeats allowed); `None` when there are none.
    pub fn new(origins: impl IntoIterator<Item = FaceOrigin>) -> Option<Side> {
        let side = Side::canonical(origins.into_iter().collect());
        (!side.0.is_empty()).then_some(side)
    }

    /// Its origins, sorted.
    pub fn origins(&self) -> &[FaceOrigin] {
        &self.0
    }

    /// The side's one origin, when it has exactly one.
    pub fn single(&self) -> Option<FaceOrigin> {
        match self.0.as_slice() {
            [o] => Some(*o),
            _ => None,
        }
    }

    /// The same side with each origin mapped by `f` (failing where `f` does).
    pub fn try_map<E>(&self, f: impl Fn(FaceOrigin) -> std::result::Result<FaceOrigin, E>) -> std::result::Result<Side, E> {
        Ok(Side::canonical(self.0.iter().map(|o| f(*o)).collect::<std::result::Result<_, E>>()?))
    }

    /// The same side with each origin's operand renumbered by `f`.
    pub fn map_operand(&self, f: impl Fn(usize) -> usize) -> Side {
        Side::canonical(self.0.iter().map(|o| o.map_operand(&f)).collect())
    }

    /// `origins` sorted, without repeats.
    fn canonical(mut origins: Vec<FaceOrigin>) -> Side {
        origins.sort_unstable();
        origins.dedup();
        Side(origins)
    }
}

impl From<FaceOrigin> for Side {
    fn from(origin: FaceOrigin) -> Side {
        Side(vec![origin])
    }
}

/// Where a result edge came from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub enum EdgeOrigin {
    /// (Part of) edge `edge` of input `operand`.
    Edge { operand: usize, edge: usize },
    /// Where two faces meet that met nowhere before (a boolean's intersection curve): the
    /// origins of the face on each side, `a <= b` (made with [`EdgeOrigin::meet`]).
    Meet { a: Side, b: Side },
}

/// A result edge: the result faces it separates and what it came from (empty when it is new
/// and lies between faces new to the operation, or between two pieces of one face: a split
/// line).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct EdgeProv {
    pub faces: Vec<usize>,
    pub origins: Vec<EdgeOrigin>,
}

/// History of an operation's result: one entry per face and per edge, in the result's orders.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct History {
    /// How many operands the operation had.
    pub operands: usize,
    pub faces: Vec<FaceProv>,
    pub edges: Vec<EdgeProv>,
}

impl FaceOrigin {
    /// The same origin with its operand (if it has one) renumbered by `f`.
    pub fn map_operand(self, f: impl Fn(usize) -> usize) -> FaceOrigin {
        match self {
            FaceOrigin::Face { operand, face } => FaceOrigin::Face { operand: f(operand), face },
            FaceOrigin::Edge { operand, edge } => FaceOrigin::Edge { operand: f(operand), edge },
            FaceOrigin::Offset { operand, face } => FaceOrigin::Offset { operand: f(operand), face },
            other => other,
        }
    }
}

impl EdgeOrigin {
    /// Where the faces with origins `a` and `b` (sides, or single origins) meet, in canonical
    /// order (the lesser side first).
    pub fn meet(a: impl Into<Side>, b: impl Into<Side>) -> EdgeOrigin {
        let (a, b) = (a.into(), b.into());
        if a <= b { EdgeOrigin::Meet { a, b } } else { EdgeOrigin::Meet { a: b, b: a } }
    }

    /// The same origin with its operands renumbered by `f`.
    pub fn map_operand(&self, f: impl Fn(usize) -> usize) -> EdgeOrigin {
        match self {
            EdgeOrigin::Edge { operand, edge } => EdgeOrigin::Edge { operand: f(*operand), edge: *edge },
            EdgeOrigin::Meet { a, b } => EdgeOrigin::meet(a.map_operand(&f), b.map_operand(&f)),
        }
    }
}

impl History {
    /// Check that this history describes a result with `result` (faces, edges) made from operands
    /// with `counts`: one entry per face and edge, and every origin within its operand's faces
    /// and edges.
    pub fn check_counts(&self, result: (usize, usize), counts: &[(usize, usize)]) -> Result<()> {
        if self.operands != counts.len() {
            return Err(failed("operand count does not match"));
        }
        if self.faces.len() != result.0 || self.edges.len() != result.1 {
            return Err(failed("entries do not match the result's faces and edges"));
        }
        let face_ok = |o: &FaceOrigin| match *o {
            FaceOrigin::Face { operand, face } | FaceOrigin::Offset { operand, face } => counts.get(operand).is_some_and(|c| face < c.0),
            FaceOrigin::Edge { operand, edge } => counts.get(operand).is_some_and(|c| edge < c.1),
            FaceOrigin::Profile { .. } | FaceOrigin::Cap { .. } => true,
        };
        if !self.faces.iter().all(|f| f.origins.iter().all(face_ok)) {
            return Err(failed("a face origin lies outside its operand"));
        }
        for e in &self.edges {
            if e.faces.iter().any(|f| *f >= self.faces.len()) {
                return Err(failed("an edge lies on a face outside the result"));
            }
            let ok = e.origins.iter().all(|o| match o {
                EdgeOrigin::Edge { operand, edge } => counts.get(*operand).is_some_and(|c| *edge < c.1),
                EdgeOrigin::Meet { a, b } => a.origins().iter().chain(b.origins()).all(face_ok),
            });
            if !ok {
                return Err(failed("an edge origin lies outside its operand"));
            }
            if e.origins.iter().any(|o| matches!(o, EdgeOrigin::Meet { a, b } if a > b)) {
                return Err(failed("a meeting's sides are out of order"));
            }
        }
        Ok(())
    }

    /// Compose with the history of the operation that made operand `k`: origins on operand `k`
    /// become that operation's origins, its operands (none, for a body made from a sketch)
    /// taking `k`'s place in the operand list; a meeting's side becomes the origins of the faces
    /// it names (several where that operation merged faces). Fails, without changing `self`,
    /// where a lineage cannot be expressed (an offset of a face that operation generated or
    /// merged, a blend of an edge it generated, a meeting with a face it made from nothing).
    pub fn substitute(&mut self, k: usize, inner: &History) -> Result<()> {
        if k >= self.operands {
            return Err(failed("no such operand"));
        }
        let n = inner.operands;
        // Operands before k stay, k's come in at k, those after k move by n - 1.
        let outer = |op: usize| if op < k { op } else { op + n - 1 };
        let lifted = |op: usize| op + k;
        let inner_face = |face: usize| inner.faces.get(face).ok_or_else(|| failed("a face origin lies outside the inner result"));
        let inner_edge = |edge: usize| inner.edges.get(edge).ok_or_else(|| failed("an edge origin lies outside the inner result"));
        // A face origin as origins of the composed operation, with the inner face's change.
        let map_face = |o: FaceOrigin| -> Result<(Vec<FaceOrigin>, Option<Change>)> {
            match o {
                FaceOrigin::Face { operand, face } if operand == k => {
                    let p = inner_face(face)?;
                    Ok((p.origins.iter().map(|x| x.map_operand(lifted)).collect(), Some(p.change)))
                }
                FaceOrigin::Offset { operand, face } if operand == k => match inner_face(face)?.origins.as_slice() {
                    [FaceOrigin::Face { operand, face }] => Ok((vec![FaceOrigin::Offset { operand: lifted(*operand), face: *face }], None)),
                    _ => Err(failed("an offset of a face the inner operation generated or merged")),
                },
                FaceOrigin::Edge { operand, edge } if operand == k => match inner_edge(edge)?.origins.as_slice() {
                    [EdgeOrigin::Edge { operand, edge }] => Ok((vec![FaceOrigin::Edge { operand: lifted(*operand), edge: *edge }], None)),
                    _ => Err(failed("a blend of an edge the inner operation generated")),
                },
                other => Ok((vec![other.map_operand(outer)], None)),
            }
        };
        let mut faces = Vec::with_capacity(self.faces.len());
        for f in &self.faces {
            let mut origins = BTreeSet::new();
            let mut kept = f.change == Change::Kept;
            for o in &f.origins {
                let (os, change) = map_face(*o)?;
                origins.extend(os);
                kept &= change.is_none_or(|c| c == Change::Kept);
            }
            let origins: Vec<FaceOrigin> = origins.into_iter().collect();
            let change = if f.change == Change::Generated || !origins.iter().any(|o| matches!(o, FaceOrigin::Face { .. })) {
                Change::Generated
            } else if origins.len() > 1 {
                Change::Merged
            } else if kept {
                Change::Kept
            } else {
                Change::Modified
            };
            faces.push(FaceProv { origins, change });
        }
        // Split where an origin lies under several composed faces (pieces the inner operation
        // split and this one joined back, or dropped all but one of, are not).
        mark_shared_split(&mut faces);
        // One side of a meeting: the origins of what its faces were made from.
        let side = |s: &Side| -> Result<Side> {
            let mut origins = Vec::new();
            for o in s.origins() {
                origins.extend(map_face(*o)?.0);
            }
            Side::new(origins).ok_or_else(|| failed("an edge meets a face the inner operation made from nothing"))
        };
        let mut edges = Vec::with_capacity(self.edges.len());
        for e in &self.edges {
            let mut origins = BTreeSet::new();
            for o in &e.origins {
                match o {
                    EdgeOrigin::Edge { operand, edge } if *operand == k => {
                        origins.extend(inner_edge(*edge)?.origins.iter().map(|x| x.map_operand(lifted)))
                    }
                    EdgeOrigin::Edge { .. } => {
                        origins.insert(o.map_operand(outer));
                    }
                    EdgeOrigin::Meet { a, b } => {
                        origins.insert(EdgeOrigin::meet(side(a)?, side(b)?));
                    }
                }
            }
            edges.push(EdgeProv { faces: e.faces.clone(), origins: origins.into_iter().collect() });
        }
        self.operands = self.operands + n - 1;
        self.faces = faces;
        self.edges = edges;
        Ok(())
    }

    /// Renumber the operands: operand `i` becomes `map[i]` of `operands` (several may become
    /// one, which joins their origins), or is dropped (`None`), which fails, without changing
    /// `self`, where an origin lies on it.
    pub fn renumber(&mut self, map: &[Option<usize>], operands: usize) -> Result<()> {
        if map.len() != self.operands || map.iter().flatten().any(|op| *op >= operands) {
            return Err(failed("operand map does not match"));
        }
        let to = |op: usize| map.get(op).copied().flatten().ok_or_else(|| failed("an origin lies on a dropped operand"));
        let face = |o: FaceOrigin| -> Result<FaceOrigin> {
            Ok(match o {
                FaceOrigin::Face { operand, face } => FaceOrigin::Face { operand: to(operand)?, face },
                FaceOrigin::Edge { operand, edge } => FaceOrigin::Edge { operand: to(operand)?, edge },
                FaceOrigin::Offset { operand, face } => FaceOrigin::Offset { operand: to(operand)?, face },
                other => other,
            })
        };
        let mut faces = Vec::with_capacity(self.faces.len());
        for f in &self.faces {
            let origins: BTreeSet<FaceOrigin> = f.origins.iter().map(|o| face(*o)).collect::<Result<_>>()?;
            // Origins that became one are one face: no longer a merge.
            let change = if f.change == Change::Merged && origins.len() == 1 { Change::Modified } else { f.change };
            faces.push(FaceProv { origins: origins.into_iter().collect(), change });
        }
        let mut edges = Vec::with_capacity(self.edges.len());
        for e in &self.edges {
            let origins: BTreeSet<EdgeOrigin> = e
                .origins
                .iter()
                .map(|o| match o {
                    EdgeOrigin::Edge { operand, edge } => Ok(EdgeOrigin::Edge { operand: to(*operand)?, edge: *edge }),
                    EdgeOrigin::Meet { a, b } => Ok(EdgeOrigin::meet(a.try_map(face)?, b.try_map(face)?)),
                })
                .collect::<Result<_>>()?;
            edges.push(EdgeProv { faces: e.faces.clone(), origins: origins.into_iter().collect() });
        }
        self.operands = operands;
        self.faces = faces;
        self.edges = edges;
        Ok(())
    }

    /// [`History::renumber`], consuming.
    pub fn renumbered(mut self, map: &[Option<usize>], operands: usize) -> Result<History> {
        self.renumber(map, operands)?;
        Ok(self)
    }

    /// [`History::substitute`], consuming.
    pub fn substituted(mut self, k: usize, inner: &History) -> Result<History> {
        self.substitute(k, inner)?;
        Ok(self)
    }

    /// This history, whose operand `i` was made by the operation `parts[i]` describes (all of
    /// them over the same operands), as a history over those operands.
    pub fn over(mut self, parts: &[&History]) -> Result<History> {
        let n = parts.first().map_or(0, |p| p.operands);
        if parts.len() != self.operands || parts.iter().any(|p| p.operands != n) {
            return Err(failed("operand histories do not match"));
        }
        // Each part's operands come in at its place: then part i's operand j is i * n + j.
        for (k, part) in parts.iter().enumerate().rev() {
            self.substitute(k, part)?;
        }
        let map: Vec<Option<usize>> = (0..self.operands).map(|op| op.checked_rem(n)).collect();
        self.renumber(&map, n)?;
        Ok(self)
    }
}

/// Faces of one origin that is under other faces too, and that still say Kept or Modified,
/// are Split (also where histories are put side by side: pieces of one body can share a
/// source).
pub fn mark_shared_split(faces: &mut [FaceProv]) {
    let mut uses: HashMap<FaceOrigin, usize> = HashMap::new();
    for o in faces.iter().flat_map(|f| &f.origins) {
        *uses.entry(*o).or_default() += 1;
    }
    for f in faces.iter_mut() {
        if matches!(f.change, Change::Kept | Change::Modified)
            && let [o] = f.origins.as_slice()
            && uses.get(o).copied().unwrap_or(0) > 1
        {
            f.change = Change::Split;
        }
    }
}
