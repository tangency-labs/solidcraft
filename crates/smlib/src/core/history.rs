//! Face and edge history: a port of the fork's `capi/src/smlib_history.cpp` (`smlib_history_tag`,
//! `smlib_history_read`) onto solidcore-ops' provenance ([`solidcore_ops::provenance`]): data on
//! the B-rep's faces and edges that the kernel's operations carry through their edits as SMLib's
//! `SmAObject::Notify` does: Split when a face or edge is cut in two, Merge when two become one,
//! Copy when an object is copied (a boolean copies the second operand into the first), Coincident
//! where a boolean finds one operand's entity lying on the other's. New topology (a boolean's
//! intersection edges) is made without any.
//!
//! The capi's `Tag` is that data ([`Provenance`]), with the capi's own rules: a split child, a
//! merge's result and the entity a coincidence lands on join the origins in, where SMLib's
//! defaults keep only one; a copy is SMLib's default. The data, the rules and the reading are the
//! capi's; ops stores it, and gives the body's topology, its edges' samples and the closest
//! points the reading compares.

use std::collections::{BTreeMap, BTreeSet};

use solidcore_ops::Order;
use solidcore_ops::measure::Adjacency;
use solidcore_ops::provenance::{Carrier, FIRST_ID, Provenance};
use solidcore_record::brep::history::Change;

use super::Body;
use super::call::{ops, ops_on};
use crate::history::{Raw, RawEdge, RawFace};
use crate::{Error, Result};

/// `TAG_ID`: above SMLib's own ids (all below `SM_AI_USER_1`).
const TAG_ID: u64 = FIRST_ID + 7;

/// Operand numbers and indices a tag accepts (`TAG_MAX`: keeps every value inside 30 bits).
const TAG_MAX: usize = 1 << 30;

/// One (operand, index) pair a face or edge stands for.
type Origin = (usize, usize);

/// The capi's `Tag`: what a face or edge stands for, the operand entities it came from, for a face
/// tagged directly its own boundary edges then (to tell a face that is still whole), and for an
/// edge tagged directly where it lay then (points along it, ends first: to tell an edge that
/// moved). Copy behaviour (`SM_AB_COPY`); a copy (`MakeCopy`) is the same data.
#[derive(Debug, Clone, PartialEq, Default)]
struct Tag {
    origins: BTreeSet<Origin>,
    edges: BTreeSet<i64>,
    at: Vec<[f64; 3]>,
}

impl Tag {
    /// `Tag::join`: `onto` (what the entity carries) with these origins and edges added, its `at`
    /// as it was; one with no `at` for an entity that carries none. Nothing for no origins.
    fn joined_onto(&self, onto: Option<&Tag>) -> Option<Tag> {
        if self.origins.is_empty() {
            return None;
        }
        let mut t = onto.cloned().unwrap_or_default();
        t.origins.extend(self.origins.iter().copied());
        t.edges.extend(self.edges.iter().copied());
        Some(t)
    }
}

impl Provenance for Tag {
    const ID: u64 = TAG_ID;

    /// A split: both children stand for what the split object stood for, and lie where it lay
    /// (each a piece of it): a child with no tag gets this one's `at` too.
    fn split(&self, child: Option<&Self>) -> Option<Self> {
        match child {
            None if !self.origins.is_empty() => Some(self.clone()),
            child => self.joined_onto(child),
        }
    }

    /// A merge: the result stands for both (the tags on the sources other than the result).
    fn merge(&self, result: Option<&Self>) -> Option<Self> {
        self.joined_onto(result)
    }

    /// One operand's entity found lying on the other's, which survives: it stands for both.
    fn coincident(&self, onto: Option<&Self>) -> Option<Self> {
        self.joined_onto(onto)
    }
}

/// Where `samples` takes points along an edge: its two ends, then three between, evenly in its
/// parameter.
const SAMPLES: [f64; 5] = [0.0, 1.0, 0.25, 0.5, 0.75];

/// Whether `p` and `q` are within `tol` (the kernel's distance; a distance it refuses is not).
fn near(p: [f64; 3], q: [f64; 3], tol: f64) -> bool {
    solidcore_ops::point_distance(p, q).is_ok_and(|d| d.value <= tol)
}

/// `moved`: have `pieces`, the result edges (indices of `order`) standing for one edge, moved from
/// where its tag says it lay (`at`, the first piece's record of at least two points)? Together
/// they must run between its ends (the vertices only one piece ends on; none for an edge closed on
/// itself, whose end some piece still ends on), and every point it ran through must lie on one of
/// them (within `tol`). One piece is the edge still whole; more, the edge cut by vertices. Pieces
/// with no record of where the edge lay have not moved.
pub(super) fn moved(body: &Body, order: &Order, adjacency: &Adjacency, pieces: &[(usize, &[[f64; 3]])], tol: f64) -> bool {
    let Some(at) = pieces.iter().map(|(_, at)| *at).find(|at| at.len() >= 2) else { return false };
    let [a, z, ..] = at else { return false };
    let mut ends: BTreeMap<usize, usize> = BTreeMap::new();
    for (e, _) in pieces {
        let Some(Some([s, f])) = adjacency.edge_vertices.get(*e) else { return true };
        *ends.entry(*s).or_default() += 1;
        *ends.entry(*f).or_default() += 1;
    }
    let point = |v: usize| adjacency.vertex_points.get(v).copied();
    let mut open = Vec::new();
    for (v, n) in &ends {
        if n % 2 == 1 {
            let Some(p) = point(*v) else { return true };
            open.push(p);
        }
    }
    let span = match open.as_slice() {
        [p, q] => (near(*p, *a, tol) && near(*q, *z, tol)) || (near(*p, *z, tol) && near(*q, *a, tol)),
        [] => near(*a, *z, tol) && ends.keys().any(|v| point(*v).is_some_and(|p| near(p, *a, tol))),
        _ => false,
    };
    if !span {
        return true;
    }
    for q in at.iter().skip(2) {
        // `SmApiEdgeClosestPoint` failing, however it fails, counts as off a piece, as a distance
        // above `tol` does: the face is then Modified, and the operation stands.
        if !pieces.iter().any(|(e, _)| body.edge_closest_point(order, *e, *q).is_ok_and(|c| c.is_some_and(|c| c.distance <= tol))) {
            return true;
        }
    }
    false
}

/// `smlib_history_tag`: a copy of `body` (`SmApiBrepCopy`) whose face i and edge i stand for face
/// i and edge i of operand `operand` (in its face and edge order, which the copy keeps), a face
/// also recording its boundary edges then, an edge the points it ran through.
pub(crate) fn tag(body: &Body, operand: usize) -> Result<Body> {
    if operand >= TAG_MAX {
        return Err(Error::Argument);
    }
    const CALL: &str = "smlib_history_tag";
    let r = ops_on(CALL, &[("brep", body)], || body.copy())?;
    let order = r.own_order();
    if order.faces.len() >= TAG_MAX || order.edges.len() >= TAG_MAX {
        return Err(Error::Argument);
    }
    let adjacency = ops_on(CALL, &[("brep", body)], || r.adjacency(&order))?;
    let samples = ops_on(CALL, &[("brep", body)], || r.edge_samples(&order, &SAMPLES))?;
    let mut data = Vec::with_capacity(order.faces.len() + order.edges.len());
    for (i, edges) in adjacency.face_edges.iter().enumerate() {
        let own: BTreeSet<i64> = edges.iter().filter_map(|j| i64::try_from(*j).ok()).collect();
        data.push((Carrier::Face(i), Tag { origins: BTreeSet::from([(operand, i)]), edges: own, at: Vec::new() }));
    }
    for (i, at) in samples.into_iter().enumerate() {
        data.push((Carrier::Edge(i), Tag { origins: BTreeSet::from([(operand, i)]), edges: BTreeSet::new(), at }));
    }
    ops_on(CALL, &[("brep", body)], || r.attach(&order, data))?;
    Ok(r)
}

/// `smlib_history_read`: each face's and edge's origins in `result`, a face's change from how
/// many origins it has, how many result faces share them and whether its boundary is still
/// exactly its origin's edges; each edge's faces. In the body's own face and edge order.
pub(crate) fn read(result: &Body) -> Result<Raw> {
    const CALL: &str = "smlib_history_read";
    let order = result.own_order();
    let tags = ops_on(CALL, &[("brep", result)], || result.carried::<Tag>(&order))?;
    let adjacency = ops_on(CALL, &[("brep", result)], || result.adjacency(&order))?;
    let tol = ops(CALL, || result.tolerance())?;

    // Each face's origins and the boundary edges they had; each edge's origins and points.
    let mut fo: Vec<BTreeSet<Origin>> = Vec::with_capacity(tags.faces.len());
    let mut fe: Vec<BTreeSet<usize>> = Vec::with_capacity(tags.faces.len());
    let mut eo: Vec<BTreeSet<Origin>> = Vec::with_capacity(tags.edges.len());
    let mut at: Vec<Vec<[f64; 3]>> = Vec::with_capacity(tags.edges.len());
    let mut face_uses: BTreeMap<Origin, usize> = BTreeMap::new();
    let mut edge_uses: BTreeMap<Origin, usize> = BTreeMap::new();
    for t in tags.faces {
        let (o, e) = t.map(|t| (t.origins, t.edges)).unwrap_or_default();
        for x in &o {
            *face_uses.entry(*x).or_default() += 1;
        }
        fe.push(e.into_iter().filter_map(|x| usize::try_from(x).ok()).collect());
        fo.push(o);
    }
    for t in tags.edges {
        let (o, p) = t.map(|t| (t.origins, t.at)).unwrap_or_default();
        for x in &o {
            *edge_uses.entry(*x).or_default() += 1;
        }
        at.push(p);
        eo.push(o);
    }

    let mut out_faces = Vec::with_capacity(fo.len());
    for (i, edges) in adjacency.face_edges.iter().enumerate() {
        let (Some(o), Some(own)) = (fo.get(i), fe.get(i)) else { continue };
        let change = match o.iter().next() {
            None => Change::Generated,
            Some(_) if o.len() > 1 => Change::Merged,
            Some(first) if face_uses.get(first).copied().unwrap_or(0) > 1 => Change::Split,
            Some(first) => {
                // Kept when its boundary is still exactly where the edges it had were: each
                // boundary edge one of its own operand's, standing for one edge only, every
                // piece of that edge on this face (an edge only cut by a vertex, as splitting
                // the face beside it cuts it, is still all there), the pieces where it lay (an
                // edge an operation moved, as a draft moves a tilted face's neighbours' edges,
                // leaves those faces trimmed differently), and those edges exactly the ones it
                // had.
                let mut pieces: BTreeMap<usize, Vec<(usize, &[[f64; 3]])>> = BTreeMap::new();
                let mut whole = true;
                for &e in edges {
                    let (Some(origins), Some(points)) = (eo.get(e), at.get(e)) else {
                        whole = false;
                        break;
                    };
                    let only = match origins.iter().next() {
                        Some(x) if origins.len() == 1 => *x,
                        _ => {
                            whole = false;
                            break;
                        }
                    };
                    whole = only.0 == first.0;
                    if !whole {
                        break;
                    }
                    let p = pieces.entry(only.1).or_default();
                    if !p.iter().any(|(x, _)| *x == e) {
                        p.push((e, points.as_slice()));
                    }
                }
                let mut now = BTreeSet::new();
                for (edge, p) in &pieces {
                    whole = whole && p.len() == edge_uses.get(&(first.0, *edge)).copied().unwrap_or(0) && !moved(result, &order, &adjacency, p, tol);
                    now.insert(*edge);
                }
                if whole && now == *own { Change::Kept } else { Change::Modified }
            }
        };
        out_faces.push(RawFace { change, origins: o.iter().copied().collect() });
    }

    let mut out_edges = Vec::with_capacity(eo.len());
    for (i, faces) in adjacency.edge_faces.iter().enumerate() {
        let bounds: BTreeSet<usize> = faces.iter().copied().collect();
        let origins = eo.get(i).map(|o| o.iter().copied().collect()).unwrap_or_default();
        out_edges.push(RawEdge { faces: bounds.into_iter().collect(), origins });
    }
    Ok(Raw { faces: out_faces, edges: out_edges })
}
