//! Face and edge history on solidcore: a port of the fork's `capi/src/smlib_history.cpp`
//! (`smlib_history_tag`, `smlib_history_read`) onto solidcore's B-rep-owned attribute store
//! ([`Brep::add_attribute`]), which solidcore's operations carry through their edits as SMLib's
//! `SmAObject::Notify` does: Split when a face or edge is cut in two, Merge when two become one,
//! Copy when an object is copied (a boolean copies the second operand into the first), Coincident
//! where a boolean finds one operand's entity lying on the other's. New topology (a boolean's
//! intersection edges) is made without any.
//!
//! The capi's `Tag` is one application attribute ([`Attribute::application`]) per face or edge,
//! with the capi's own handlers ([`AttributeHandlers`]): a split child, a merge's result and the
//! entity a coincidence lands on join the origins in, where SMLib's defaults keep only one; a copy
//! is SMLib's default. The data, the handlers and the reading are the capi's.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use solidcore::geom::attribute::{
    ApplicationValue, Attribute, AttributeHandle, AttributeHandlers, AttributeValue, AttributeView, Behavior, ObjectKey, Placement, ids,
};
use solidcore::kernel::Kernel;
use solidcore::query::api_queries::edge_closest_point;
use solidcore::topology::face::p4 as face4;
use solidcore::topology::{Brep, EdgeId, VertexId, edge};
use solidcore::{EntityRef, Point3, SolidError};
use solidcraft_geom::brep::history::Change;

use super::{Body, answer, with_body};
use crate::history::{Raw, RawEdge, RawFace};
use crate::{Error, Result};
use solidcore::topology::brep::p3::brep_copy;

/// `TAG_ID`: above SMLib's own ids (all below `SM_AI_USER_1`).
const TAG_ID: u64 = ids::USER_1 + 7;

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
    at: Vec<Point3>,
}

impl Tag {
    fn attribute(self) -> Attribute {
        Attribute::application(TAG_ID, Behavior::Copy, self)
    }
}

/// `Tag::on`: the tag on `x`, if the attribute of its id there is a [`Tag`].
fn on<'a>(view: &AttributeView<'a>, x: ObjectKey) -> Option<(AttributeHandle, &'a Tag)> {
    let (h, a) = view.find_attribute(x, TAG_ID)?;
    a.application_value::<Tag>().map(|t| (h, t))
}

/// `Tag::join`: adds `o` and `e` to `x`'s tag (its `at` as it was), giving it one with no `at` if
/// it has none (an attribute of the tag's id that is no tag is replaced). Nothing for no `o`.
fn join(view: &AttributeView<'_>, x: ObjectKey, o: &BTreeSet<Origin>, e: &BTreeSet<i64>) -> Vec<Placement> {
    if o.is_empty() {
        return Vec::new();
    }
    if let Some((handle, t)) = on(view, x) {
        let mut t = t.clone();
        t.origins.extend(o.iter().copied());
        t.edges.extend(e.iter().copied());
        return vec![Placement::Edit { handle, value: AttributeValue::Application(ApplicationValue::new(t)) }];
    }
    set(view, x, o.clone(), e.clone(), Vec::new())
}

/// `Tag::set`: replaces whatever tag `x` has (the attribute of the tag's id) with exactly `o`, `e`
/// and `p`.
fn set(view: &AttributeView<'_>, x: ObjectKey, o: BTreeSet<Origin>, e: BTreeSet<i64>, p: Vec<Point3>) -> Vec<Placement> {
    let mut out = Vec::with_capacity(2);
    if let Some((handle, _)) = view.find_attribute(x, TAG_ID) {
        out.push(Placement::Remove { on: x, handle });
    }
    out.push(Placement::Add { on: x, attribute: Tag { origins: o, edges: e, at: p }.attribute() });
    out
}

impl AttributeHandlers for Tag {
    /// A split: both children stand for what the split object stood for, and lie where it lay
    /// (each a piece of it): a child with no tag gets this one's `at` too.
    fn split(&self, view: &AttributeView<'_>, split: ObjectKey, a: ObjectKey, b: ObjectKey) -> solidcore::Result<Vec<Placement>> {
        let mut out = Vec::new();
        for x in [a, b] {
            if x == split {
                continue;
            }
            if on(view, x).is_none() && !self.origins.is_empty() {
                out.extend(set(view, x, self.origins.clone(), self.edges.clone(), self.at.clone()));
            } else {
                out.extend(join(view, x, &self.origins, &self.edges));
            }
        }
        Ok(out)
    }

    /// A merge: the result stands for both (the tags on the sources other than the result).
    fn merge(&self, view: &AttributeView<'_>, a: ObjectKey, b: ObjectKey, result: ObjectKey) -> solidcore::Result<Vec<Placement>> {
        let (mut o, mut e) = (BTreeSet::new(), BTreeSet::new());
        for x in [a, b] {
            if x != result
                && let Some((_, t)) = on(view, x)
            {
                o.extend(t.origins.iter().copied());
                e.extend(t.edges.iter().copied());
            }
        }
        Ok(join(view, result, &o, &e))
    }

    /// One operand's entity found lying on the other's, which survives: it stands for both.
    fn coincident(&self, view: &AttributeView<'_>, _from: ObjectKey, to: ObjectKey) -> solidcore::Result<Vec<Placement>> {
        Ok(join(view, to, &self.origins, &self.edges))
    }
}

/// `read`: what `e`'s tag says (the attribute of the tag's id, if it is a [`Tag`]).
fn tag_on(b: &Brep, e: EntityRef) -> Option<&Tag> {
    let (_, a) = b.entity_attributes(e).into_iter().find(|(_, a)| a.attribute_id() == TAG_ID)?;
    a.application_value::<Tag>()
}

/// `Tag::set` at tagging: replaces whatever tag `e` has with exactly origin `o`, the boundary
/// edges `edges` (a face's) and the points `at` (an edge's).
fn set_on(b: &mut Brep, e: EntityRef, o: Origin, edges: BTreeSet<i64>, at: Vec<Point3>) -> std::result::Result<(), SolidError> {
    let old = b.entity_attributes(e).into_iter().find(|(_, a)| a.attribute_id() == TAG_ID).map(|(h, _)| h);
    if let Some(h) = old {
        b.remove_attribute(e, h)?;
    }
    b.add_attribute(e, Tag { origins: BTreeSet::from([o]), edges, at }.attribute())?;
    Ok(())
}

/// `samples`: points along `e`, its two ends, then three between, evenly in its parameter. Empty
/// where it can't be evaluated.
fn samples(k: &Kernel, b: &Brep, e: EdgeId) -> std::result::Result<Vec<Point3>, SolidError> {
    let edge = b.edge(e)?;
    let Some(curve) = edge.curve.as_ref() else { return Ok(Vec::new()) };
    let (lo, hi) = (edge.interval.min(), edge.interval.max());
    let mut p = Vec::with_capacity(5);
    for t in [0.0, 1.0, 0.25, 0.5, 0.75] {
        let Some(v) = answer(curve.evaluate(k, lo + t * (hi - lo), 0, true, false))? else { return Ok(Vec::new()) };
        let Some(v) = v.first() else { return Ok(Vec::new()) };
        p.push(Point3::new(v.x, v.y, v.z));
    }
    Ok(p)
}

/// `moved`: have `pieces`, the result edges standing for one edge, moved from where its tag says
/// it lay (`at`, the first piece's record of at least two points)? Together they must run between
/// its ends (the vertices only one piece ends on; none for an edge closed on itself, whose end some
/// piece still ends on), and every point it ran through must lie on one of them (within `tol`).
/// One piece is the edge still whole; more, the edge cut by vertices. Pieces with no record of
/// where the edge lay have not moved.
pub(super) fn moved(k: &Kernel, b: &Brep, pieces: &[(EdgeId, &[Point3])], tol: f64) -> std::result::Result<bool, SolidError> {
    let Some(at) = pieces.iter().map(|(_, at)| *at).find(|at| at.len() >= 2) else { return Ok(false) };
    let [a, z, ..] = at else { return Ok(false) };
    let near = |p: &Point3, q: &Point3| p.distance(*q) <= tol;
    let mut ends: BTreeMap<VertexId, usize> = BTreeMap::new();
    for (e, _) in pieces {
        let (Some(s), Some(f)) = edge::start_end_vertices(b, *e)? else { return Ok(true) };
        *ends.entry(s).or_default() += 1;
        *ends.entry(f).or_default() += 1;
    }
    let mut open = Vec::new();
    for (v, n) in &ends {
        if n % 2 == 1 {
            open.push(b.vertex(*v)?.point);
        }
    }
    let span = match open.as_slice() {
        [p, q] => (near(p, a) && near(q, z)) || (near(p, z) && near(q, a)),
        [] => {
            let mut on_end = false;
            for v in ends.keys() {
                on_end = on_end || near(&b.vertex(*v)?.point, a);
            }
            near(a, z) && on_end
        }
        _ => false,
    };
    if !span {
        return Ok(true);
    }
    for q in at.iter().skip(2) {
        // `SmApiEdgeClosestPoint` failing, however it fails, counts as off a piece, as a distance
        // above `tol` does: the face is then Modified, and the operation stands.
        if !pieces.iter().any(|(e, _)| edge_closest_point(k, b, *e, *q).is_ok_and(|c| c.closest.distance <= tol)) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `smlib_history_tag`: a copy of `body` (`SmApiBrepCopy`) whose face i and edge i stand for face
/// i and edge i of operand `operand` (in its face and edge order, which the copy keeps), a face
/// also recording its boundary edges then, an edge the points it ran through.
pub(crate) fn tag(body: &Body, operand: usize) -> Result<Body> {
    if operand >= TAG_MAX {
        return Err(Error::Argument);
    }
    with_body("smlib_history_tag", body, |k, b| {
        let mut r = brep_copy(k, b)?;
        let faces = r.face_list().to_vec();
        let edges = r.edge_list().to_vec();
        if faces.len() >= TAG_MAX || edges.len() >= TAG_MAX {
            return Ok(Err(Error::Argument));
        }
        let edge_at: HashMap<EdgeId, usize> = edges.iter().enumerate().map(|(i, e)| (*e, i)).collect();
        for (i, f) in faces.iter().enumerate() {
            let own: BTreeSet<i64> =
                face4::edges(&r, *f, None, false, None)?.iter().filter_map(|e| edge_at.get(e).and_then(|j| i64::try_from(*j).ok())).collect();
            set_on(&mut r, EntityRef::Face(*f), (operand, i), own, Vec::new())?;
        }
        for (i, e) in edges.iter().enumerate() {
            let at = samples(k, &r, *e)?;
            set_on(&mut r, EntityRef::Edge(*e), (operand, i), BTreeSet::new(), at)?;
        }
        Ok(Ok(r))
    })?
    .map(Body::new)
}

/// `smlib_history_read`: each face's and edge's origins in `result`, a face's change from how
/// many origins it has, how many result faces share them and whether its boundary is still
/// exactly its origin's edges; each edge's faces. In the body's own face and edge order.
pub(crate) fn read(result: &Body) -> Result<Raw> {
    with_body("smlib_history_read", result, |k, b| {
        let faces = b.face_list().to_vec();
        let edges = b.edge_list().to_vec();
        let face_at: HashMap<_, usize> = faces.iter().enumerate().map(|(i, f)| (*f, i)).collect();
        let edge_at: HashMap<EdgeId, usize> = edges.iter().enumerate().map(|(i, e)| (*e, i)).collect();
        let tol = b.tolerance();

        // Each face's origins and the boundary edges they had; each edge's origins and points.
        let mut fo: Vec<BTreeSet<Origin>> = Vec::with_capacity(faces.len());
        let mut fe: Vec<BTreeSet<usize>> = Vec::with_capacity(faces.len());
        let mut eo: Vec<BTreeSet<Origin>> = Vec::with_capacity(edges.len());
        let mut at: Vec<Vec<Point3>> = Vec::with_capacity(edges.len());
        let mut face_uses: BTreeMap<Origin, usize> = BTreeMap::new();
        let mut edge_uses: BTreeMap<Origin, usize> = BTreeMap::new();
        for f in &faces {
            let (o, e) = tag_on(b, EntityRef::Face(*f)).map(|t| (t.origins.clone(), t.edges.clone())).unwrap_or_default();
            for x in &o {
                *face_uses.entry(*x).or_default() += 1;
            }
            fe.push(e.into_iter().filter_map(|x| usize::try_from(x).ok()).collect());
            fo.push(o);
        }
        for e in &edges {
            let (o, p) = tag_on(b, EntityRef::Edge(*e)).map(|t| (t.origins.clone(), t.at.clone())).unwrap_or_default();
            for x in &o {
                *edge_uses.entry(*x).or_default() += 1;
            }
            at.push(p);
            eo.push(o);
        }

        let mut out_faces = Vec::with_capacity(faces.len());
        for (i, f) in faces.iter().enumerate() {
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
                    let mut pieces: BTreeMap<usize, Vec<(EdgeId, &[Point3])>> = BTreeMap::new();
                    let mut whole = true;
                    for e in face4::edges(b, *f, None, false, None)? {
                        let Some(j) = edge_at.get(&e).copied() else {
                            whole = false;
                            break;
                        };
                        let (Some(origins), Some(points)) = (eo.get(j), at.get(j)) else {
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
                        whole = whole && p.len() == edge_uses.get(&(first.0, *edge)).copied().unwrap_or(0) && !moved(k, b, p, tol)?;
                        now.insert(*edge);
                    }
                    if whole && now == *own { Change::Kept } else { Change::Modified }
                }
            };
            out_faces.push(RawFace { change, origins: o.iter().copied().collect() });
        }

        let mut out_edges = Vec::with_capacity(edges.len());
        for (i, e) in edges.iter().enumerate() {
            let bounds: BTreeSet<usize> = edge::faces(b, *e, None)?.iter().filter_map(|f| face_at.get(f).copied()).collect();
            let origins = eo.get(i).map(|o| o.iter().copied().collect()).unwrap_or_default();
            out_edges.push(RawEdge { faces: bounds.into_iter().collect(), origins });
        }
        Ok(Raw { faces: out_faces, edges: out_edges })
    })
}
