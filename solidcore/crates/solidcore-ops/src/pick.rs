//! Picks on a body: the face a point lies on, and the faces a pick grows to across the edges
//! where the kernel proves the faces on both sides continue one surface (a hole's wall stored
//! in pieces opens whole). Every decision is the kernel's: the point classification
//! (`SmBrep::Point3DClassify`) and [`solidcore::query::faces_continue`]. Ops chooses and
//! sequences.

use std::collections::BTreeMap;

use solidcore::error::EntityRef;
use solidcore::intersect::point_class::PointClassType;
use solidcore::query::brep::BrepClassifier;
use solidcore::query::faces_continue::{faces_continue, Continues};
use solidcore::topology::{edge, face, EdgeId, FaceId};
use solidcore::Point3;

use crate::body::Body;
use crate::order::{self, Order};
use crate::session::{with_body, Answer};
use crate::{Error, Result};

impl Answer for Vec<FaceId> {}

impl Body {
    /// The record face (through `order`) point `p` lies on, within `reach` of it, or `None`:
    /// the kernel's classification of the point against the body at that tolerance
    /// (`SmBrep::Point3DClassify`, with boundary intersections). A point the kernel puts on an
    /// edge or a vertex is on more than one face and picks none; so does a point off the body or
    /// one the kernel does not classify. Never a face guessed by nearness.
    ///
    /// # Errors
    /// [`Error::Argument`] for a coordinate or a reach that is not finite and positive; the
    /// kernel's status (its classification may add trim curves to the body).
    pub fn face_at(&self, order: &Order, p: [f64; 3], reach: f64) -> Result<Option<usize>> {
        if !(p.iter().all(|x| x.is_finite()) && reach.is_finite() && reach > 0.0) {
            return Err(Error::Argument);
        }
        let point = Point3::new(p[0], p[1], p[2]);
        let found = with_body("face_at", self, |k, b| {
            let mut classifier = BrepClassifier::of_brep(b);
            let pc = match classifier.point_3d_classify(k, point, reach, true) {
                Ok(pc) => pc,
                Err(e) if e.sm_status().is_some() => return Ok(None),
                Err(e) => return Err(e),
            };
            Ok(match (pc.point_class(), pc.object()) {
                (PointClassType::Face, Some(EntityRef::Face(f))) => Some(f),
                _ => None,
            })
        })?;
        Ok(found.and_then(|f| order.faces.iter().position(|&x| x == f)))
    }

    /// Record faces `faces` (through `order`) and every face joined to them, across an edge not
    /// in `keep_edges` (record edge indices: the edges that stay apart, such as a split face's
    /// line), where the kernel proves the two faces continue one surface with the same outward
    /// side ([`faces_continue`]). An edge the kernel leaves undecided is not crossed. The picks
    /// first, in their order, then each face as it is reached.
    ///
    /// # Errors
    /// [`Error::Argument`] for an index past the faces or the edges; the kernel's (a side it is
    /// unsure is material, a dangling id).
    pub fn faces_on_one_surface(
        &self,
        order: &Order,
        faces: &[usize],
        keep_edges: &[usize],
    ) -> Result<Vec<usize>> {
        let seeds: Vec<FaceId> = faces
            .iter()
            .map(|&i| order::face(order, i))
            .collect::<Result<_>>()?;
        let keep: Vec<EdgeId> = keep_edges
            .iter()
            .map(|&i| order::edge(order, i))
            .collect::<Result<_>>()?;
        let index: BTreeMap<FaceId, usize> = order
            .faces
            .iter()
            .enumerate()
            .map(|(i, &f)| (f, i))
            .collect();
        let grown = with_body("faces_on_one_surface", self, |_, b| {
            let mut out: Vec<FaceId> = Vec::new();
            for f in seeds {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
            let mut next = 0;
            while let Some(&f) = out.get(next) {
                next += 1;
                for e in face::edges(b, f, None, false, None)? {
                    if keep.contains(&e) {
                        continue;
                    }
                    for g in edge::faces(b, e, None)? {
                        if g != f
                            && !out.contains(&g)
                            && faces_continue(b, f, g, e)? == Continues::Proved
                        {
                            out.push(g);
                        }
                    }
                }
            }
            Ok(out)
        })?;
        grown
            .into_iter()
            .map(|f| index.get(&f).copied().ok_or(Error::Argument))
            .collect()
    }
}
