//! Face and edge history out of SMLib operations, in [`solidcraft_geom::brep::history`]'s terms.
//!
//! SMLib carries identity through its own edits: every split, merge, copy and coincidence an
//! operation makes passes the attributes of what it changes on to what it makes. So each operand
//! is tagged ([`Brep::tagged`]: face i and edge i become operand k's face i and edge i), the
//! operation runs on the tagged copies, and the result's tags say where each face and edge came
//! from ([`Brep::history`]). Nothing is sampled or guessed.
//!
//! Indices here are SMLib's own: positions in a body's face and edge lists, which a copy keeps.
//! [`Raw::history`] renumbers them into canonical order (docs/native-brep.md) with the
//! [`Order`] the export gives each body.

use std::collections::BTreeSet;

use solidcraft_geom::brep::history::{Change, EdgeOrigin, EdgeProv, FaceOrigin, FaceProv, Failed, History, Side};

use crate::{Brep, Error, Result, error, ffi};

/// A result face as SMLib reports it: how it relates to its origins, and the (operand, face)
/// pairs it came from, sorted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawFace {
    pub change: Change,
    pub origins: Vec<(usize, usize)>,
}

/// A result edge as SMLib reports it: the result faces it bounds (ascending) and the (operand,
/// edge) pairs it came from, sorted; none where it is new (where its faces meet).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawEdge {
    pub faces: Vec<usize>,
    pub origins: Vec<(usize, usize)>,
}

/// A result's history in SMLib's own face and edge order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raw {
    pub faces: Vec<RawFace>,
    pub edges: Vec<RawEdge>,
}

/// Where one body's faces and edges go in canonical order: `faces[i]` is the canonical index of
/// SMLib's face i, likewise for edges. Empty means SMLib's order is the canonical one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Order {
    pub faces: Vec<usize>,
    pub edges: Vec<usize>,
}

impl Brep {
    /// A copy whose face i and edge i are tagged as face i and edge i of operand `operand` (in
    /// SMLib's order, which the copy keeps). Run an operation on tagged copies, then read its
    /// result's [`Brep::history`].
    pub fn tagged(&self, operand: usize) -> Result<Brep> {
        let operand = i32::try_from(operand).map_err(|_| Error::Argument)?;
        ffi::history::tag(&self.0, operand).map(Brep).map_err(error)
    }

    /// The history of this body, made by an operation from tagged operands: each face's and
    /// edge's origins among them. Faces and edges no tag reached have none.
    pub fn history(&self) -> Result<Raw> {
        let v = ffi::history::read(&self.0).map_err(error)?;
        Raw::decode(&v).ok_or(Error::Exception)
    }
}

impl Raw {
    /// Decodes `smlib_history_read`'s records; `None` where they don't read as its layout.
    fn decode(v: &[i64]) -> Option<Raw> {
        let mut r = Reader(v.iter());
        if r.next()? != usize::try_from(ffi::history::VERSION).ok()? {
            return None;
        }
        let (nf, ne) = (r.next()?, r.next()?);
        // Every entry takes at least two values: no more entries than values.
        if nf.saturating_add(ne) > v.len() {
            return None;
        }
        let mut faces = Vec::with_capacity(nf);
        for _ in 0..nf {
            let change = match r.next()? {
                0 => Change::Kept,
                1 => Change::Modified,
                2 => Change::Split,
                3 => Change::Merged,
                4 => Change::Generated,
                _ => return None,
            };
            let n = r.next()?;
            faces.push(RawFace { change, origins: r.pairs(n)? });
        }
        let mut edges = Vec::with_capacity(ne);
        for _ in 0..ne {
            let m = r.next()?;
            let bounds = (0..m).map(|_| r.next()).collect::<Option<Vec<_>>>()?;
            let n = r.next()?;
            edges.push(RawEdge { faces: bounds, origins: r.pairs(n)? });
        }
        r.0.next().is_none().then_some(Raw { faces, edges })
    }

    /// As a [`History`] over `operands.len()` operands, every index renumbered into canonical
    /// order: operand k's through `operands[k]`, the result's through `result`. An edge with no
    /// origin is where its two faces meet; one bounding other than two faces, or a face new to
    /// the operation, has no origin at all.
    pub fn history(&self, operands: &[Order], result: &Order) -> std::result::Result<History, Failed> {
        let bad = |what: &'static str| Failed(what);
        let map = |order: &[usize], i: usize| -> std::result::Result<usize, Failed> {
            if order.is_empty() { Ok(i) } else { order.get(i).copied().ok_or(bad("an index lies outside its body's order")) }
        };
        let operand = |k: usize| operands.get(k).ok_or(bad("an origin lies on no operand"));
        let face_origin = |(k, f): (usize, usize)| -> std::result::Result<FaceOrigin, Failed> {
            let o = operand(k)?;
            Ok(FaceOrigin::Face { operand: k, face: map(&o.faces, f)? })
        };
        // The result's faces and edges, each at its canonical place.
        let place = |order: &[usize], n: usize| -> std::result::Result<Vec<usize>, Failed> {
            let at: Vec<usize> = (0..n).map(|i| map(order, i)).collect::<std::result::Result<_, _>>()?;
            let distinct: BTreeSet<usize> = at.iter().copied().collect();
            if distinct.len() != n || at.iter().any(|c| *c >= n) {
                return Err(bad("the result's order is no permutation"));
            }
            Ok(at)
        };
        let (fat, eat) = (place(&result.faces, self.faces.len())?, place(&result.edges, self.edges.len())?);

        let mut faces: Vec<Option<FaceProv>> = vec![None; self.faces.len()];
        for (f, at) in self.faces.iter().zip(&fat) {
            let origins: BTreeSet<FaceOrigin> = f.origins.iter().map(|o| face_origin(*o)).collect::<std::result::Result<_, _>>()?;
            faces[*at] = Some(FaceProv { origins: origins.into_iter().collect(), change: f.change });
        }
        let faces: Vec<FaceProv> = faces.into_iter().collect::<Option<_>>().ok_or(bad("the result's order is no permutation"))?;

        let mut edges: Vec<Option<EdgeProv>> = vec![None; self.edges.len()];
        for (e, at) in self.edges.iter().zip(&eat) {
            let bounds: BTreeSet<usize> = e
                .faces
                .iter()
                .map(|f| fat.get(*f).copied().ok_or(bad("an edge lies on a face outside the result")))
                .collect::<std::result::Result<_, _>>()?;
            let bounds: Vec<usize> = bounds.into_iter().collect();
            let origins = if e.origins.is_empty() {
                // New: where its two faces meet, each side what that face came from.
                let side = |f: usize| faces.get(f).and_then(|p| Side::new(p.origins.iter().copied()));
                match bounds.as_slice() {
                    [a, b] => side(*a).zip(side(*b)).map(|(a, b)| EdgeOrigin::meet(a, b)).into_iter().collect(),
                    _ => Vec::new(),
                }
            } else {
                let set: BTreeSet<EdgeOrigin> = e
                    .origins
                    .iter()
                    .map(|(k, i)| {
                        let o = operand(*k)?;
                        Ok(EdgeOrigin::Edge { operand: *k, edge: map(&o.edges, *i)? })
                    })
                    .collect::<std::result::Result<_, Failed>>()?;
                set.into_iter().collect()
            };
            edges[*at] = Some(EdgeProv { faces: bounds, origins });
        }
        let edges: Vec<EdgeProv> = edges.into_iter().collect::<Option<_>>().ok_or(bad("the result's order is no permutation"))?;
        Ok(History { operands: operands.len(), faces, edges })
    }
}

/// A blend's faces (fillet, chamfer) in `h`, the history of a blend of edges `blended` of its
/// one operand (operand 0), whose own edges `operand` lists (the faces each bounds, as in the
/// operand's identity history): SMLib makes a blend face with no tag, so each Generated face is
/// given [`FaceOrigin::Edge`] for every blended edge whose two faces both lie under faces next
/// to it and no longer meet each other directly. A corner patch between several blends matches
/// none of them and stays without origin.
pub fn blends(h: &mut History, operand: &[EdgeProv], blended: &[usize]) {
    let mut next_to: Vec<BTreeSet<FaceOrigin>> = vec![BTreeSet::new(); h.faces.len()];
    for e in &h.edges {
        for f in &e.faces {
            for g in &e.faces {
                if let (Some(set), Some(p)) = (next_to.get_mut(*f), h.faces.get(*g))
                    && f != g
                {
                    set.extend(p.origins.iter().copied());
                }
            }
        }
    }
    // Pairs of operand faces whose result faces still meet along an edge.
    let mut meeting: BTreeSet<(FaceOrigin, FaceOrigin)> = BTreeSet::new();
    for e in &h.edges {
        if let [a, b] = e.faces.as_slice()
            && let (Some(a), Some(b)) = (h.faces.get(*a), h.faces.get(*b))
        {
            for x in &a.origins {
                for y in &b.origins {
                    meeting.insert((*x.min(y), *x.max(y)));
                }
            }
        }
    }
    for (f, around) in h.faces.iter_mut().zip(&next_to) {
        if f.change != Change::Generated || !f.origins.is_empty() {
            continue;
        }
        let edges = blended.iter().filter(|e| {
            operand.get(**e).is_some_and(|p| {
                let [a, b] = p.faces.as_slice() else { return false };
                let (a, b) = (FaceOrigin::Face { operand: 0, face: *a }, FaceOrigin::Face { operand: 0, face: *b });
                around.contains(&a) && around.contains(&b) && !meeting.contains(&(a.min(b), a.max(b)))
            })
        });
        f.origins = edges.map(|e| FaceOrigin::Edge { operand: 0, edge: *e }).collect();
    }
}

/// Reads non-negative values off the records.
struct Reader<'a>(std::slice::Iter<'a, i64>);

impl Reader<'_> {
    fn next(&mut self) -> Option<usize> {
        self.0.next().and_then(|x| usize::try_from(*x).ok())
    }

    /// `n` pairs; `None` past the end (so a huge `n` costs nothing).
    fn pairs(&mut self, n: usize) -> Option<Vec<(usize, usize)>> {
        let mut v = Vec::with_capacity(n.min(self.0.len() / 2));
        for _ in 0..n {
            v.push((self.next()?, self.next()?));
        }
        Some(v)
    }
}

#[cfg(test)]
mod tests;
