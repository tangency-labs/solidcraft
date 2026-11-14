//! Tangent chains on record order: the edges a pick runs on into tangentially, and the faces
//! joined to picked faces across smooth edges. The walks are here; every measure they stop
//! on is the kernel's: whether an edge's faces meet tangentially all along it
//! ([`solidcore::query::tangent_edge`]), how far one edge turns from running straight on into
//! another at a vertex and whether an edge is convex there ([`solidcore::query::edge_turn`]).
//!
//! The caller's `angle` (radians) is the one tolerance: edges meeting at less than it continue
//! one another, and faces meeting at less than it all along an edge make it smooth. Nothing
//! here picks one (SolidCraft passes its shaded view's half a degree).
//!
//! - [`crate::blend::tangent_chain`] (its walk is here): the picked edges, then every edge
//!   that continues one of them
//!   within `angle` at a shared vertex, and so on. An edge does not join when it is not
//!   between two faces (a one-face edge), when it is smooth (its faces meet tangentially all
//!   along it, a seam among them), or when it turns over (convex where the edge it continues
//!   is concave, or the reverse).
//!
//!   The kernel's own blend walk (`blend::blend_refusal::tangent_chain`, `continuing_edges`) is
//!   a different job and stays beside it: which edges a blend of an edge runs on into at the
//!   blend corner, from the edge's blend edge use, as fillet construction asks it.
//! - [`tangent_face_chain_ordered`]: the picked faces, then every face across a smooth edge
//!   from one of them, and so on.
//! - [`curve_chain_ordered`]: the picked edges, then every edge that continues one of them
//!   within `angle` at a shared vertex, as curves alone: a path, a rail or a boundary to run
//!   along, not a corner to blend. The faces play no part (a one-face edge or a smooth edge
//!   joins like any other); the walk stops at a vertex where more than one edge continues
//!   (a branch: which way the path goes is not the walk's to choose).
//!
//! An edge whose smoothness the kernel leaves undecided (a blend's net within its margin of
//! the angle, [`solidcore::query::tangent_edge::Tangency::Undecided`]) joins nothing and is
//! named in the answer.

use std::collections::{BTreeMap, BTreeSet};

use solidcore::query::edge_turn::{convex_at, turn_at};
use solidcore::query::tangent_edge::{tangent_all_along, Tangency};
use solidcore::topology::{edge, face, vertex};
use solidcore::{Brep, EdgeId, FaceId, Kernel};

use crate::session::with_body;
use crate::{Body, Error, Order, Result};

/// What [`crate::blend::tangent_chain`] and [`tangent_face_chain_ordered`] answer, in record
/// indices.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Chain {
    /// The picks (each once, in order), then what joined them, in the order it joined.
    pub members: Vec<usize>,
    /// Edges the walk met whose smoothness the kernel left undecided: they joined nothing.
    pub undecided: Vec<usize>,
}

/// A kernel edge's smoothness, decided once.
struct Smooth {
    known: BTreeMap<EdgeId, Option<Tangency>>,
}

impl Smooth {
    fn of(
        &mut self,
        k: &Kernel,
        b: &mut Brep,
        e: EdgeId,
        angle: f64,
    ) -> solidcore::Result<Option<Tangency>> {
        if let Some(t) = self.known.get(&e) {
            return Ok(*t);
        }
        let t = tangent_all_along(k, b, e, angle)?;
        self.known.insert(e, t);
        Ok(t)
    }
}

/// An angle a chain takes: finite, strictly between 0 and a right angle.
fn angle_ok(angle: f64) -> bool {
    angle > 0.0 && angle < std::f64::consts::FRAC_PI_2
}

fn index_of<T: PartialEq + Copy>(list: &[T], x: T) -> Option<usize> {
    list.iter().position(|&y| y == x)
}

/// The edge walk of [`crate::blend::tangent_chain`] on `b`, `list` its edges in record order.
pub(crate) fn edge_walk(
    k: &Kernel,
    b: &mut Brep,
    list: &[EdgeId],
    edges: &[usize],
    passed_over: &[usize],
    angle: f64,
) -> solidcore::Result<Result<Chain>> {
    if edges.is_empty() || !angle_ok(angle) {
        return Ok(Err(Error::Argument));
    }
    let mut chain: Vec<EdgeId> = Vec::new();
    for &i in edges {
        match list.get(i) {
            Some(&e) if b.edge_list().contains(&e) => {
                if !chain.contains(&e) {
                    chain.push(e);
                }
            }
            _ => return Ok(Err(Error::Argument)),
        }
    }
    let mut skip: BTreeSet<EdgeId> = BTreeSet::new();
    for &i in passed_over {
        match list.get(i) {
            Some(&e) if b.edge_list().contains(&e) => {
                skip.insert(e);
            }
            _ => return Ok(Err(Error::Argument)),
        }
    }
    // each edge's record index, for the candidates' order
    let index: BTreeMap<EdgeId, usize> = list.iter().enumerate().map(|(i, &e)| (e, i)).collect();
    let mut smooth = Smooth {
        known: BTreeMap::new(),
    };
    let mut undecided: Vec<EdgeId> = Vec::new();
    let mut next = 0;
    while let Some(&e) = chain.get(next) {
        next += 1;
        let ends = [edge::start_vertex(b, e)?, edge::end_vertex(b, e)?];
        for v in ends.into_iter().flatten() {
            // the edges at the vertex, in record order
            let mut at: Vec<EdgeId> = vertex::edges(b, v, None)?;
            at.sort_by_key(|f| index.get(f).copied().unwrap_or(usize::MAX));
            at.dedup();
            for f in at {
                if chain.contains(&f) || skip.contains(&f) {
                    continue;
                }
                // the cheap questions first: does it continue this edge, and on the same
                // side; only then is its smoothness certified
                if !turn_at(k, b, e, f, v)?.is_some_and(|turn| turn < angle) {
                    continue;
                }
                let turns_over = matches!(
                    (convex_at(k, b, e, v, angle)?, convex_at(k, b, f, v, angle)?),
                    (Some(a), Some(c)) if a != c
                );
                if turns_over {
                    continue;
                }
                match smooth.of(k, b, f, angle)? {
                    Some(Tangency::NotTangent) => chain.push(f),
                    Some(Tangency::Undecided { .. }) => {
                        if !undecided.contains(&f) {
                            undecided.push(f);
                        }
                    }
                    None | Some(Tangency::Tangent) => {}
                }
            }
        }
    }
    let indices =
        |of: &[EdgeId]| -> Option<Vec<usize>> { of.iter().map(|&e| index_of(list, e)).collect() };
    Ok(match (indices(&chain), indices(&undecided)) {
        (Some(members), Some(undecided)) => Ok(Chain { members, undecided }),
        _ => Err(Error::Exception),
    })
}

/// The edges of `body` at record indices `edges` (through `order`), each once in order, then
/// every edge that continues one of them within `angle` radians at a shared vertex, and so on
/// (module documentation): the curve chain a path, rail or boundary pick stands for. At a
/// vertex where more than one other edge continues the edge arriving there the chain does not
/// go on (a branch). A closed edge (a whole circle) continues nothing. Every measure is the
/// kernel's ([`solidcore::query::edge_turn::turn_at`]). [`Chain::undecided`] stays empty.
///
/// # Errors
/// [`Error::Argument`] for no edges, an angle as for [`crate::blend::tangent_chain`], or an
/// index past the order or naming an edge the body does not have; the kernel's refusals (an
/// edge's curve that cannot be evaluated).
pub fn curve_chain_ordered(
    body: &Body,
    order: &Order,
    edges: &[usize],
    angle: f64,
) -> Result<Chain> {
    if edges.is_empty() || !angle_ok(angle) {
        return Err(Error::Argument);
    }
    let (copy, mapped) = body.copy_with_order(order)?;
    with_body("curve_chain_ordered", &copy, |k, b| {
        let list = mapped.edges.clone();
        let mut chain: Vec<EdgeId> = Vec::new();
        for &i in edges {
            match list.get(i) {
                Some(&e) if b.edge_list().contains(&e) => {
                    if !chain.contains(&e) {
                        chain.push(e);
                    }
                }
                _ => return Ok(Err(Error::Argument)),
            }
        }
        let index: BTreeMap<EdgeId, usize> =
            list.iter().enumerate().map(|(i, &e)| (e, i)).collect();
        let mut next = 0;
        while let Some(&e) = chain.get(next) {
            next += 1;
            let ends = [edge::start_vertex(b, e)?, edge::end_vertex(b, e)?];
            if ends[0].is_some() && ends[0] == ends[1] {
                continue; // a closed edge: no end to run on from
            }
            for v in ends.into_iter().flatten() {
                // the edges at the vertex, in record order
                let mut at: Vec<EdgeId> = vertex::edges(b, v, None)?;
                at.sort_by_key(|f| index.get(f).copied().unwrap_or(usize::MAX));
                at.dedup();
                let mut onward: Vec<EdgeId> = Vec::new();
                for f in at {
                    if f != e && turn_at(k, b, e, f, v)?.is_some_and(|turn| turn < angle) {
                        onward.push(f);
                    }
                }
                if let [f] = onward[..] {
                    if !chain.contains(&f) {
                        chain.push(f);
                    }
                }
            }
        }
        let members: Option<Vec<usize>> = chain.iter().map(|&e| index_of(&list, e)).collect();
        Ok(match members {
            Some(members) => Ok(Chain {
                members,
                undecided: Vec::new(),
            }),
            None => Err(Error::Exception),
        })
    })?
}

/// The faces of `body` at record indices `faces` (through `order`), then every face across a
/// smooth edge from one of them, and so on (module documentation), at the caller's `angle`.
///
/// # Errors
/// [`Error::Argument`] for no faces, an angle as for [`crate::blend::tangent_chain`], or an
/// index past the order or naming a face the body does not have; the kernel's refusals as for
/// [`crate::blend::tangent_chain`].
pub fn tangent_face_chain_ordered(
    body: &Body,
    order: &Order,
    faces: &[usize],
    angle: f64,
) -> Result<Chain> {
    if faces.is_empty() || !angle_ok(angle) {
        return Err(Error::Argument);
    }
    let (copy, mapped) = body.copy_with_order(order)?;
    with_body("tangent_face_chain_ordered", &copy, |k, b| {
        let face_list = mapped.faces.clone();
        let edge_list = mapped.edges.clone();
        let mut chain: Vec<FaceId> = Vec::new();
        for &i in faces {
            match face_list.get(i) {
                Some(&f) if b.face_list().contains(&f) => {
                    if !chain.contains(&f) {
                        chain.push(f);
                    }
                }
                _ => return Ok(Err(Error::Argument)),
            }
        }
        let index: BTreeMap<EdgeId, usize> =
            edge_list.iter().enumerate().map(|(i, &e)| (e, i)).collect();
        let mut smooth = Smooth {
            known: BTreeMap::new(),
        };
        let mut undecided: Vec<EdgeId> = Vec::new();
        let mut next = 0;
        while let Some(&f) = chain.get(next) {
            next += 1;
            // the face's edges, in record order
            let mut around = face::p4::edges(b, f, None, false, None)?;
            around.sort_by_key(|e| index.get(e).copied().unwrap_or(usize::MAX));
            around.dedup();
            for e in around {
                let across = solidcore::query::edge::edge_faces(b, e)?;
                match smooth.of(k, b, e, angle)? {
                    Some(Tangency::Tangent) => {}
                    Some(Tangency::Undecided { .. }) => {
                        if !undecided.contains(&e) {
                            undecided.push(e);
                        }
                        continue;
                    }
                    None | Some(Tangency::NotTangent) => continue,
                }
                for g in across {
                    if !chain.contains(&g) {
                        chain.push(g);
                    }
                }
            }
        }
        let members: Option<Vec<usize>> = chain.iter().map(|&f| index_of(&face_list, f)).collect();
        let undecided: Option<Vec<usize>> =
            undecided.iter().map(|&e| index_of(&edge_list, e)).collect();
        Ok(match (members, undecided) {
            (Some(members), Some(undecided)) => Ok(Chain { members, undecided }),
            _ => Err(Error::Exception),
        })
    })?
}
