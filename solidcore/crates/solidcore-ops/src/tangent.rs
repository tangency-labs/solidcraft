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
//! - [`tangent_chain_ordered`]: the picked edges, then every edge that continues one of them
//!   within `angle` at a shared vertex, and so on. An edge does not join when it is not
//!   between two faces (a one-face edge), when it is smooth (its faces meet tangentially all
//!   along it, a seam among them), or when it turns over (convex where the edge it continues
//!   is concave, or the reverse).
//! - [`tangent_face_chain_ordered`]: the picked faces, then every face across a smooth edge
//!   from one of them, and so on.
//!
//! An edge whose smoothness the kernel leaves undecided (a blend's net within its margin of
//! the angle, [`solidcore::query::tangent_edge::Tangency::Undecided`]) joins nothing and is
//! named in the answer.

use solidcore::query::edge_turn::{convex_at, turn_at};
use solidcore::query::tangent_edge::{tangent_all_along, Tangency};
use solidcore::topology::edge;
use solidcore::{Brep, EdgeId, FaceId, Kernel};

use crate::session::with_body;
use crate::{Body, Error, Order, Result};

/// What [`tangent_chain_ordered`] and [`tangent_face_chain_ordered`] answer, in record
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
    known: Vec<(EdgeId, Option<Tangency>)>,
}

impl Smooth {
    fn of(
        &mut self,
        k: &Kernel,
        b: &mut Brep,
        e: EdgeId,
        angle: f64,
    ) -> solidcore::Result<Option<Tangency>> {
        if let Some((_, t)) = self.known.iter().find(|(f, _)| *f == e) {
            return Ok(*t);
        }
        let t = tangent_all_along(k, b, e, angle)?;
        self.known.push((e, t));
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

/// The tangent chain of `body`'s edges at record indices `edges` (through `order`, its
/// record's [`Order`]), as the module documentation describes, at the caller's `angle`;
/// `passed_over` names edges a pick passes over, which never join.
///
/// # Errors
/// [`Error::Argument`] for no edges, an angle not finite and strictly between 0 and a right
/// angle, or an index past the order or naming an edge the body does not have; the kernel's
/// refusals (a face with no NURBS form, an edge use with no pcurve, an edge too costly to
/// decide).
pub fn tangent_chain_ordered(
    body: &Body,
    order: &Order,
    edges: &[usize],
    passed_over: &[usize],
    angle: f64,
) -> Result<Chain> {
    if edges.is_empty() || !angle_ok(angle) {
        return Err(Error::Argument);
    }
    let (copy, mapped) = body.copy_with_order(order)?;
    with_body("tangent_chain_ordered", &copy, |k, b| {
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
        let skip: Vec<EdgeId> = passed_over
            .iter()
            .filter_map(|&i| list.get(i).copied())
            .collect();
        let mut smooth = Smooth { known: Vec::new() };
        let mut undecided: Vec<EdgeId> = Vec::new();
        let mut next = 0;
        while let Some(&e) = chain.get(next) {
            next += 1;
            let ends = [edge::start_vertex(b, e)?, edge::end_vertex(b, e)?];
            for v in ends.into_iter().flatten() {
                for &f in &list {
                    if chain.contains(&f) || skip.contains(&f) {
                        continue;
                    }
                    let meets = [edge::start_vertex(b, f)?, edge::end_vertex(b, f)?];
                    if !meets.contains(&Some(v)) {
                        continue;
                    }
                    match smooth.of(k, b, f, angle)? {
                        None | Some(Tangency::Tangent) => continue,
                        Some(Tangency::Undecided { .. }) => {
                            if !undecided.contains(&f) {
                                undecided.push(f);
                            }
                            continue;
                        }
                        Some(Tangency::NotTangent) => {}
                    }
                    let turns_over = matches!(
                        (convex_at(k, b, e, v)?, convex_at(k, b, f, v)?),
                        (Some(a), Some(c)) if a != c
                    );
                    if turns_over {
                        continue;
                    }
                    if turn_at(k, b, e, f, v)?.is_some_and(|turn| turn < angle) {
                        chain.push(f);
                    }
                }
            }
        }
        let indices = |of: &[EdgeId]| -> Option<Vec<usize>> {
            of.iter().map(|&e| index_of(&list, e)).collect()
        };
        Ok(match (indices(&chain), indices(&undecided)) {
            (Some(members), Some(undecided)) => Ok(Chain { members, undecided }),
            _ => Err(Error::Exception),
        })
    })?
}

/// The faces of `body` at record indices `faces` (through `order`), then every face across a
/// smooth edge from one of them, and so on (module documentation), at the caller's `angle`.
///
/// # Errors
/// [`Error::Argument`] for no faces, an angle as for [`tangent_chain_ordered`], or an index
/// past the order or naming a face the body does not have; the kernel's refusals as for
/// [`tangent_chain_ordered`].
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
        let mut smooth = Smooth { known: Vec::new() };
        let mut undecided: Vec<EdgeId> = Vec::new();
        let mut next = 0;
        while let Some(&f) = chain.get(next) {
            next += 1;
            for &e in &edge_list {
                let across = solidcore::query::edge::edge_faces(b, e)?;
                if !across.contains(&f) {
                    continue;
                }
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
